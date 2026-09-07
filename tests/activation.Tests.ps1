#requires -Version 7.4
# Real direct links + native Codex TOML editor; only package acquisition/discovery
# and neutral core startup are fixtures. Never mutates the user's installations.
[CmdletBinding()]
param([string]$CrashRoot, [string]$CrashAt = 'registration')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$original = Get-Content (Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex/harness/installation.json') -Raw | ConvertFrom-Json
$native = @(Get-ChildItem (Join-Path (Split-Path $original.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor') -Filter codex.exe -File -Recurse)[0].FullName
$python = Join-Path ([Environment]::GetFolderPath('UserProfile')) 'AppData/Roaming/uv/tools/serena-agent/Scripts/python.exe'
$suite = if ($CrashRoot) { $CrashRoot } else { Join-Path ([IO.Path]::GetTempPath()) ('harness-activation-' + [guid]::NewGuid().ToString('N')) }
$initialPath = $env:Path
$assertions = 0
$cases = 0
function Assert-True([bool]$Value, [string]$Message) { if (-not $Value) { throw $Message }; $script:assertions++ }
function Assert-Throws([scriptblock]$Action, [string]$Pattern) {
    $errorText = $null
    try { & $Action | Out-Null } catch { $errorText = $_.Exception.Message }
    Assert-True ($errorText -and $errorText -match $Pattern) "Expected /$Pattern/: $errorText"
}
function Setup-Modules {
    $script:core = Import-Module (Join-Path $repository 'tools/kit.psm1') -Force -PassThru
    & $core {
        function script:Assert-HarnessPrerequisites { @{ codex = 'fixture'; openspec = 'fixture'; powershell = '7.6.5' } }
        function script:Test-HarnessRuntime { @{ evidence = 'Lifecycle fixture; real neutral startup tested by installer.Tests.ps1.' } }
    }
    $script:code = Import-Module (Join-Path $repository 'tools/code-tools.psm1') -Force -PassThru
    & $code {
        param($Python, $Native, $Repository)
        $script:fixturePython = $Python; $script:fixtureNative = $Native
        $script:fixtureGraphifyHelper = Join-Path $Repository 'tests/fixtures/code-tools-native/graphify-update-fixture.py'
        $script:fixtureGraphifyJournal = $null
        $script:realPythonJson = (Get-Command Invoke-CodeToolsPythonJson).ScriptBlock
        function script:Get-HarnessCodeToolsRuntime {
            param($UserHome, $CodexHome, $CodexCommand)
            @{ python = $script:fixturePython; lifecycle_python = $script:fixturePython; native = $script:fixtureNative; powershell = (Get-Command pwsh).Source; registry = (Join-Path $CodexHome 'harness/code-tools.json') }
        }
        function script:Invoke-CodeToolsPythonJson {
            param($Python, [string[]]$Arguments)
            switch ([IO.Path]::GetFileName($Arguments[0])) {
                'dependencies.py' {
                    if ($Arguments[1] -eq 'recover' -and $script:fixtureGraphifyJournal) {
                        return & $script:realPythonJson $Python @($script:fixtureGraphifyHelper, 'recover', '--journal', $script:fixtureGraphifyJournal)
                    }
                    return @{ complete = $true; results = @(@{ id = 'fixture-packages'; state = 'reused' }) }
                }
                'discovery.py' { return @{ schema_version = 1; mcp = @(); languages = @(); fixture = 'new-inventory' } }
                'registry.py' { return @{ schema_version = 1; servers = @{ fixture = @{ command = @('not-launched') } } } }
                'check.py' { return @{ status = 'protocol-ready'; evidence = 'fixture only' } }
            }
            & $script:realPythonJson $Python $Arguments
        }
    } $python $native $repository
    $script:activation = Import-Module (Join-Path $repository 'tools/activation.psm1') -Force -PassThru
    & $activation {
        function script:Initialize-CodeToolsRuntime {}
        # Subscription routing has its own real-link/recovery fixtures. This
        # regression suite must never install a package or launch a live proxy.
        function script:Invoke-HarnessSubscriptionRouting { @{ status = 'ready'; evidence = 'subscription fixture' } }
        function script:Restore-HarnessSubscriptionRouting {}
        function script:Resume-HarnessSubscriptionRouting {}
    }
}
function New-Fixture([string]$Name) {
    $root = Join-Path $suite $Name
    @{ SourceRoot = $repository; UserHome = (Join-Path $root 'user'); CodexHome = (Join-Path $root 'codex'); CodexCommand = $original.codexCommand; PathScope = 'Process' }
}
function Seed-Fixture($Fixture) {
    $fixtureHome = $Fixture.CodexHome
    Write-CodeToolsBytes (Join-Path $fixtureHome 'config.toml') ([Text.Encoding]::UTF8.GetBytes('# exact foreign bytes' + [char]10 + 'model = "gpt-6-astra"' + [char]10))
    Write-CodeToolsJson (Join-Path $fixtureHome 'harness/code-tools.json') @{ mcp = @(); fixture = 'old-inventory' }
    Write-CodeToolsJson (Join-Path $fixtureHome 'harness/lsp-servers.json') @{ servers = @{}; fixture = 'old-lsp' }
}
function Snapshot($Fixture) {
    $result = @{}
    foreach ($relative in @('config.toml','harness/installation.json','harness/code-tools-registration.json','harness/code-tools.json','harness/lsp-servers.json')) {
        $path = Join-Path $Fixture.CodexHome $relative
        $result[$relative] = if (Test-Path -LiteralPath $path) { [Convert]::ToBase64String((Get-CodeToolsBytes $path)) } else { $null }
    }
    $result
}
function Assert-Restored($Fixture, $Before, [string]$BeforePath) {
    $after = Snapshot $Fixture
    foreach ($name in $Before.Keys) { Assert-True ($Before[$name] -ceq $after[$name]) "Exact prior bytes/absence not restored: $name" }
    Assert-True ($env:Path -ceq $BeforePath) 'PATH did not restore.'
    Assert-True (-not (Test-Path (Join-Path $Fixture.CodexHome 'harness/activation-pending.json'))) 'Outer transaction did not clear.'
}
Setup-Modules
if ($CrashRoot) {
    $fixture = New-Fixture 'crash'
    Seed-Fixture $fixture
    Invoke-HarnessActivation @fixture -Mode Install -Checkpoint { param($Phase) if ($Phase -eq $CrashAt) { [Environment]::Exit(86) } } | Out-Null
    throw 'Crash checkpoint was not reached.'
}
try {
    foreach ($phase in @('core','registration','registry:code-tools.json','registry:lsp-servers.json','before-commit')) {
        $fixture = New-Fixture ($phase.Replace(':','-'))
        Seed-Fixture $fixture
        $before = Snapshot $fixture; $beforePath = $env:Path
        Assert-Throws { Invoke-HarnessActivation @fixture -Mode Install -Checkpoint { param($value) if ($value -eq $phase) { throw "injected $phase" } } } 'injected'
        Assert-Restored $fixture $before $beforePath
        Assert-True (-not (Test-Path (Join-Path $fixture.UserHome '.agents/skills/openspec-explore'))) 'A core skill link remained after rollback.'
        $cases++; Write-Output "PASS: rollback after $phase"
    }
    $additiveFixture = New-Fixture 'additive-effect'
    Seed-Fixture $additiveFixture
    Assert-Throws { Invoke-HarnessActivation @additiveFixture -Mode Install -Checkpoint {
        param($value)
        if ($value -eq 'core') {
            $pending = Read-CodeToolsJson (Join-Path $additiveFixture.CodexHome 'harness/activation-pending.json')
            Write-CodeToolsJson (Join-Path $additiveFixture.CodexHome ('harness/dependencies/result-' + $pending.id + '.json')) @{ results = @(@{ id = 'unrecorded-addition-fixture'; state = 'installed-unverified' }) }
            throw 'additive effect fixture failure'
        }
    } } 'changed without a rollback journal'
    Assert-True (-not (Test-Path (Join-Path $additiveFixture.CodexHome 'harness/installation.json'))) 'Independent core restore was blocked by additive effect metadata.'
    $pending = Read-CodeToolsJson (Join-Path $additiveFixture.CodexHome 'harness/activation-pending.json')
    Assert-True ($pending.phase -eq 'incompleteUpdate') 'Unreversed additive effect incorrectly reported complete rollback.'
    # The fixture effect is metadata only. Resolve that exact simulated effect.
    Write-CodeToolsJson (Join-Path $additiveFixture.CodexHome ('harness/dependencies/result-' + $pending.id + '.json')) @{ results = @(@{ id = 'resolved-fixture'; state = 'reused' }) }
    Invoke-HarnessActivation @additiveFixture -Mode Recover | Out-Null
    Assert-True (-not (Test-Path (Join-Path $additiveFixture.CodexHome 'harness/activation-pending.json'))) 'Resolved fixture recovery remained pending.'
    $cases++; Write-Output 'PASS: additive effects without an inverse remain visibly incomplete'
    $graphifyFixture = New-Fixture 'graphify-result-contract'
    Seed-Fixture $graphifyFixture
    $graphifyBefore = Snapshot $graphifyFixture; $graphifyPathBefore = $env:Path
    Assert-Throws { Invoke-HarnessActivation @graphifyFixture -Mode Install -Checkpoint {
        param($value)
        if ($value -eq 'before-commit') {
            $pending = Read-CodeToolsJson (Join-Path $graphifyFixture.CodexHome 'harness/activation-pending.json')
            $promoted = & $code {
                param($UserPath, $StatePath, $Transaction)
                $result = & $script:realPythonJson $script:fixturePython @($script:fixtureGraphifyHelper, 'promote', '--user-home', $UserPath, '--state-dir', $StatePath, '--transaction-id', $Transaction)
                $script:fixtureGraphifyJournal = $result.transaction_journal
                $result
            } $graphifyFixture.UserHome (Join-Path $graphifyFixture.CodexHome 'harness/dependencies') $pending.id
            Write-CodeToolsJson (Join-Path $graphifyFixture.CodexHome ('harness/dependencies/result-' + $pending.id + '.json')) @{ results = @($promoted) }
            throw 'later failure after Graphify update'
        }
    } } 'later failure after Graphify update'
    Assert-Restored $graphifyFixture $graphifyBefore $graphifyPathBefore
    Assert-True ((Get-Content (Join-Path $graphifyFixture.UserHome 'AppData/Roaming/uv/tools/graphifyy/version.txt') -Raw) -eq 'old') 'Graphify actual helper result did not permit directory rollback.'
    Invoke-HarnessActivation @graphifyFixture -Mode Recover | Out-Null
    & $code { $script:fixtureGraphifyJournal = $null }
    $cases++; Write-Output 'PASS: actual Graphify result contract and later failure complete coordinator recovery'
    $ownerFixture = New-Fixture 'separate-dependency-owner'
    $ownerFixture.DependencyUserHome = Join-Path $suite 'explicit-shared-owner'
    Seed-Fixture $ownerFixture
    Invoke-HarnessActivation @ownerFixture -Mode Install | Out-Null
    Assert-True ((Read-CodeToolsJson (Join-Path $ownerFixture.CodexHome 'harness/installation.json')).dependencyUserHome -eq $ownerFixture.DependencyUserHome) 'Explicit dependency owner was not persisted.'
    $ownerBefore = Snapshot $ownerFixture; $ownerPathBefore = $env:Path
    $wrongOwner = $ownerFixture.Clone(); $wrongOwner.DependencyUserHome = $ownerFixture.UserHome
    Assert-Throws { Invoke-HarnessActivation @wrongOwner -Mode Check } 'Dependency owner differs'
    Assert-Throws { Invoke-HarnessActivation @ownerFixture -Mode Update -Checkpoint {
        param($value)
        if ($value -eq 'core') {
            $pendingPath = Join-Path $ownerFixture.CodexHome 'harness/activation-pending.json'
            $pending = Read-CodeToolsJson $pendingPath
            Assert-True ($pending.dependency_user_home -eq $ownerFixture.DependencyUserHome) 'Pending activation omitted its explicit dependency owner.'
            $pendingHash = Get-CodeToolsHash (Get-CodeToolsBytes $pendingPath)
            Assert-Throws { Restore-HarnessActivation @wrongOwner -Preview } 'Dependency owner differs'
            Assert-True ((Get-CodeToolsHash (Get-CodeToolsBytes $pendingPath)) -eq $pendingHash) 'Wrong-owner recovery changed its pending journal.'
            throw 'explicit owner rollback'
        }
    } } 'explicit owner rollback'
    Assert-Restored $ownerFixture $ownerBefore $ownerPathBefore
    Invoke-HarnessActivation @ownerFixture -Mode Disconnect | Out-Null
    $cases++; Write-Output 'PASS: explicit dependency owner persists through rollback and rejects wrong-owner Check/Recover'
    $readinessFixture = New-Fixture 'legacy-readiness'
    Seed-Fixture $readinessFixture
    $readinessOriginal = Get-CodeToolsBytes (Join-Path $readinessFixture.CodexHome 'config.toml')
    Invoke-HarnessActivation @readinessFixture -Mode Install | Out-Null
    $readinessStatePath = Join-Path $readinessFixture.CodexHome 'harness/code-tools-registration.json'
    $readinessState = Read-CodeToolsJson $readinessStatePath
    $readinessStatement = [string]$readinessState.connection_policy.statement
    $readinessState.Remove('connection_policy')
    Write-CodeToolsJson $readinessStatePath $readinessState
    $legacyText = [Text.Encoding]::UTF8.GetString((Get-CodeToolsBytes (Join-Path $readinessFixture.CodexHome 'config.toml'))).Replace($readinessStatement, '')
    Write-CodeToolsBytes (Join-Path $readinessFixture.CodexHome 'config.toml') ([Text.Encoding]::UTF8.GetBytes($legacyText))
    $legacyCheck = Invoke-HarnessActivation @readinessFixture -Mode Check
    Assert-True ($legacyCheck.status -eq 'Degraded' -and $legacyCheck.codeTools.status -eq 'degraded') 'Protocol-ready health concealed missing native readiness registration.'
    Invoke-HarnessActivation @readinessFixture -Mode Install | Out-Null
    Assert-True ((Invoke-HarnessActivation @readinessFixture -Mode Check).status -eq 'Connected') 'Native readiness migration did not restore connected Check.'
    Invoke-HarnessActivation @readinessFixture -Mode Disconnect | Out-Null
    Assert-True ((Get-CodeToolsHash (Get-CodeToolsBytes (Join-Path $readinessFixture.CodexHome 'config.toml'))) -eq (Get-CodeToolsHash $readinessOriginal)) 'Readiness migration Disconnect did not restore prior config bytes.'
    $cases++; Write-Output 'PASS: legacy native readiness migration is visibly degraded until installed and restores prior absence'
    $fixture = New-Fixture 'update'
    Seed-Fixture $fixture
    Invoke-HarnessActivation @fixture -Mode Install | Out-Null
    $before = Snapshot $fixture; $beforePath = $env:Path
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Update -Checkpoint { param($value) if ($value -eq 'registry:lsp-servers.json') { throw 'update failure' } } } 'update failure'
    Assert-Restored $fixture $before $beforePath
    $cases++; Write-Output 'PASS: update failure preserves pre-existing links and exact state'
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Update -Checkpoint {
        param($value)
        if ($value -eq 'core') {
            $statePath = Join-Path $fixture.CodexHome 'harness/installation.json'
            $edited = Read-CodeToolsJson $statePath; $edited['foreignMarker'] = 'keep'
            Write-CodeToolsJson $statePath $edited
            throw 'state conflict injection'
        }
    } } 'Installation state changed'
    Assert-True ((Read-CodeToolsJson (Join-Path $fixture.CodexHome 'harness/installation.json')).foreignMarker -eq 'keep') 'Concurrent core state was overwritten.'
    Write-CodeToolsBytes (Join-Path $fixture.CodexHome 'harness/installation.json') ([Convert]::FromBase64String($before['harness/installation.json']))
    Invoke-HarnessActivation @fixture -Mode Recover | Out-Null
    Assert-Restored $fixture $before $beforePath
    $cases++; Write-Output 'PASS: concurrent core metadata preserved and recovery resumed after resolution'
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Update -Checkpoint {
        param($value)
        if ($value -eq 'registry:code-tools.json') {
            Add-Content -LiteralPath (Join-Path $fixture.CodexHome 'harness/lsp-servers.json') ' '
            throw 'registry conflict injection'
        }
    } } 'Registry changed after interruption'
    Assert-True ((Get-CodeToolsHash (Get-CodeToolsBytes (Join-Path $fixture.CodexHome 'harness/lsp-servers.json'))) -ne (Get-CodeToolsHash ([Convert]::FromBase64String($before['harness/lsp-servers.json'])))) 'Concurrent registry bytes were overwritten.'
    Write-CodeToolsBytes (Join-Path $fixture.CodexHome 'harness/lsp-servers.json') ([Convert]::FromBase64String($before['harness/lsp-servers.json']))
    Invoke-HarnessActivation @fixture -Mode Recover | Out-Null
    Assert-Restored $fixture $before $beforePath
    $cases++; Write-Output 'PASS: both registry records retain concurrent-change protection'
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Disconnect -Checkpoint { param($value) if ($value -eq 'before-commit') { throw 'disconnect failure' } } } 'disconnect failure'
    Assert-Restored $fixture $before $beforePath
    $cases++; Write-Output 'PASS: disconnect failure restores core and native registrations'

    # An intervening config edit is never overwritten. Other components still recover.
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Disconnect -Checkpoint {
        param($value)
        if ($value -eq 'before-commit') {
            Add-Content -LiteralPath (Join-Path $fixture.CodexHome 'config.toml') '# intervening edit'
            throw 'conflict injection'
        }
    } } 'Recovery incomplete'
    Assert-True ((Get-Content (Join-Path $fixture.CodexHome 'config.toml') -Raw).Contains('intervening edit')) 'Concurrent config bytes were lost.'
    Assert-True ((Read-CodeToolsJson (Join-Path $fixture.CodexHome 'harness/activation-pending.json')).phase -eq 'incompleteUpdate') 'Conflict was not durably visible.'
    Assert-True (Test-Path (Join-Path $fixture.UserHome '.agents/skills/openspec-explore')) 'Independent core recovery did not run.'
    # Resolve only the exact fixture edit; an explicit Recover can now finish.
    $registrationPending = Read-CodeToolsJson (Join-Path $fixture.CodexHome 'harness/code-tools-registration-pending.json')
    Write-CodeToolsBytes (Join-Path $fixture.CodexHome 'config.toml') ([Convert]::FromBase64String($registrationPending.before))
    Invoke-HarnessActivation @fixture -Mode Recover | Out-Null
    Assert-Restored $fixture $before $beforePath
    $cases++; Write-Output 'PASS: concurrent edit preserved, independent recovery continued, explicit resolution recovered'

    # The actual native writer can normalize and interleave registration tables.
    # Disconnect must still work when only the adopted Serena environment is gone.
    $nativeEnvironment = $env:CODEX_HOME
    try {
        $env:CODEX_HOME = $fixture.CodexHome
        & $native mcp add foreign-normalized -- foreign-normalized.exe | Out-Null
        Assert-True ($LASTEXITCODE -eq 0) 'Native normalization failed.'
    } finally { $env:CODEX_HOME = $nativeEnvironment }
    & $code {
        $script:fixturePython = (& (Get-Command uv).Source python find --no-project --managed-python --offline --no-python-downloads '>=3.11')
        function script:Get-HarnessCodeToolsRuntime {
            param($UserHome, $CodexHome, $CodexCommand)
            @{ python = $null; lifecycle_python = $script:fixturePython; native = $script:fixtureNative; powershell = (Get-Command pwsh).Source; registry = (Join-Path $CodexHome 'harness/code-tools.json') }
        }
    }
    $normalizedBefore = Snapshot $fixture
    Assert-Throws { Invoke-HarnessActivation @fixture -Mode Disconnect -Checkpoint { param($value) if ($value -eq 'before-commit') { throw 'normalized disconnect rollback' } } } 'normalized disconnect rollback'
    Assert-Restored $fixture $normalizedBefore $beforePath
    Invoke-HarnessActivation @fixture -Mode Disconnect | Out-Null
    Assert-True ((Get-Content (Join-Path $fixture.CodexHome 'config.toml') -Raw).Contains('[mcp_servers.foreign-normalized]')) 'Normalized foreign MCP table was removed.'
    $cases++; Write-Output 'PASS: normalized native config disconnect and rollback use stdlib base Python without Serena'
    # Reconnect the fixture to retain the separate all-Python-unavailable case.
    Setup-Modules
    Invoke-HarnessActivation @fixture -Mode Install | Out-Null

    # Recovery and disconnect never call a Python runtime.
    & $code { function script:Get-HarnessCodeToolsRuntime { throw 'Python must not be resolved for native disconnection/recovery' } }
    Invoke-HarnessActivation @fixture -Mode Disconnect | Out-Null
    Assert-True (-not (Test-Path (Join-Path $fixture.CodexHome 'harness/installation.json'))) 'No-Python disconnect did not remove core state.'
    Assert-True ((Get-CodeToolsBytes (Join-Path $fixture.CodexHome 'config.toml')).Length -gt 0) 'Foreign config was lost.'
    Assert-True (-not ([Text.Encoding]::UTF8.GetString((Get-CodeToolsBytes (Join-Path $fixture.CodexHome 'config.toml')))).Contains('mcp_optional_startup_grace_ms')) 'No-Python Disconnect left the owned native readiness scalar.'
    Invoke-HarnessActivation @fixture -Mode Recover | Out-Null
    $cases++; Write-Output 'PASS: Disconnect/Recover do not require Python'
    $env:Path = $initialPath
    Setup-Modules

    foreach ($crashPhase in @('registration','registry:code-tools.json','committed')) {
        $childRoot = Join-Path $suite ('hard-' + $crashPhase.Replace(':','-'))
        $start = [Diagnostics.ProcessStartInfo]::new((Get-Command pwsh).Source)
        foreach ($arg in @('-NoLogo','-NoProfile','-File',$PSCommandPath,'-CrashRoot',$childRoot,'-CrashAt',$crashPhase)) { $start.ArgumentList.Add($arg) }
        $start.UseShellExecute = $false; $start.CreateNoWindow = $true
        $process = [Diagnostics.Process]::Start($start)
        try { Assert-True ($process.WaitForExit(30000)) 'Hard-crash fixture timed out.'; Assert-True ($process.ExitCode -eq 86) 'Child did not reach crash checkpoint.' } finally { if (-not $process.HasExited) { $process.Kill($true) }; $process.Dispose() }
        $crashed = @{ SourceRoot = $repository; UserHome = (Join-Path $childRoot 'crash/user'); CodexHome = (Join-Path $childRoot 'crash/codex'); CodexCommand = $original.codexCommand; PathScope = 'Process' }
        Assert-True (Test-Path (Join-Path $crashed.CodexHome 'harness/activation-pending.json')) 'Crash lost durable outer journal.'
        Invoke-HarnessActivation @crashed -Mode Recover | Out-Null
        if ($crashPhase -eq 'committed') {
            Assert-True (Test-Path (Join-Path $crashed.CodexHome 'harness/installation.json')) 'Committed activation was incorrectly rolled back.'
            Assert-True ((Get-Content (Join-Path $crashed.CodexHome 'config.toml') -Raw).Contains('BEGIN codex-harness')) 'Committed MCP config was lost.'
        } else {
            Assert-True (-not (Test-Path (Join-Path $crashed.CodexHome 'harness/installation.json'))) 'Uncommitted core state remained.'
            Assert-True (-not (Get-Content (Join-Path $crashed.CodexHome 'config.toml') -Raw).Contains('BEGIN codex-harness')) 'Uncommitted MCP config remained.'
            Assert-True ((Read-CodeToolsJson (Join-Path $crashed.CodexHome 'harness/code-tools.json')).fixture -eq 'old-inventory') 'Prior inventory was not restored.'
        }
        Assert-True (-not (Test-Path (Join-Path $crashed.CodexHome 'harness/activation-pending.json'))) 'Recovered hard crash remained pending.'
        $cases++; Write-Output "PASS: hard crash at $crashPhase"
    }
    Write-Output "Activation checks passed: $cases scenarios, $assertions assertions. Evidence fixtures: $suite"
} finally { $env:Path = $initialPath }
