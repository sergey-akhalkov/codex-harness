#requires -Version 7.4
# Real temporary links/files. Package, runtime and lifecycle use fixtures;
# one unique native task exercises XML registration without ever running.
# The Windows Job Object itself is tested by subscription-process.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot
Import-Module (Join-Path $repository 'tools/code-tools.psm1')
$suite = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-routing-' + [guid]::NewGuid().ToString('N'))
$module = Import-Module (Join-Path $repository 'tools/subscription-routing.psm1') -Force -PassThru
$assertions = 0
function Assert-True([bool]$Value,[string]$Message) { if (-not $Value) { throw $Message }; $script:assertions++ }
function Assert-Throw([scriptblock]$Action,[string]$Pattern) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($caught -and $caught -match $Pattern) "Expected /$Pattern/, got: $caught"
}
function New-Fixture {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Creates only disposable suite state; transactional tests require deterministic setup.')]
    param([string]$Name)
    $root = Join-Path $suite $Name
    $fixture = @{ SourceRoot = Join-Path $root 'source'; UserHome = Join-Path $root 'user'; CodexHome = Join-Path $root 'codex'; CodexCommand = 'C:\fixture\codex.ps1' }
    [void][IO.Directory]::CreateDirectory((Join-Path $fixture.SourceRoot 'global/opencodex/agents'))
    Write-CodeToolsJson (Join-Path $fixture.SourceRoot 'global/opencodex/config.json') @{ hostname='127.0.0.1';port=10100;codexAutoStart=$false;codexShimAutoRestore=$false }
    Write-CodeToolsBytes (Join-Path $fixture.CodexHome 'config.toml') ([Text.Encoding]::UTF8.GetBytes("# foreign original`r`nmodel = `"gpt-6-astra`"`r`n"))
    Write-CodeToolsJson (Join-Path $fixture.UserHome '.opencodex/auth.json') @{ fixture = 'private-auth-preserved' }
    $fixture
}
function Snapshot($Fixture) {
    $state = @{}
    foreach ($relative in @('config.toml','harness/subscription-routing.json','harness/subscriptions/service.json')) {
        $path = Join-Path $Fixture.CodexHome $relative
        $state[$relative] = if (Test-Path -LiteralPath $path) { [Convert]::ToBase64String((Get-CodeToolsBytes $path)) } else { $null }
    }
    $state
}
function Assert-Snapshot($Fixture,$Before) {
    $after = Snapshot $Fixture
    foreach ($key in $Before.Keys) { Assert-True ($Before[$key] -ceq $after[$key]) "Prior bytes/absence not restored: $key" }
}
& $module {
    $script:tasks = @{}; $script:descriptors = @{}; $script:started = @{}; $script:missingPackage = $false; $script:failReady = $false
    $script:nativeRestoreCalls = 0; $script:boundedServiceCalls = 0
    $script:taskSetCalls = 0; $script:taskStartCalls = 0
    $script:realReady = (Get-Command Test-SubscriptionReady).ScriptBlock
    $script:realWaitReady = (Get-Command Wait-SubscriptionReady).ScriptBlock
    $script:taskSamples = [Collections.Generic.Queue[hashtable]]::new()
    $script:realNativeRestore = (Get-Command Restore-SubscriptionNative).ScriptBlock
    $script:realStarted = (Get-Command Get-SubscriptionStarted).ScriptBlock
    $script:realBounded = (Get-Command Invoke-SubscriptionBounded).ScriptBlock
    $script:realOwnedProcess = (Get-Command Get-SubscriptionOwnedProcess).ScriptBlock
    $script:useStopFixture = $false
    $script:useActualReceipt = $false
    $script:realTaskRead = (Get-Command Get-SubscriptionTask).ScriptBlock
    $script:realTaskXml = (Get-Command New-SubscriptionTaskXml).ScriptBlock
    $script:realTaskFolder = (Get-Command Get-SubscriptionTaskFolder).ScriptBlock
    $script:failCleanup = $false
    function script:Get-SubscriptionDependency {
        if ($script:missingPackage) { return $null }
        @{ root='C:\fixture\package';bun='C:\fixture\bun.exe';cli='C:\fixture\index.ts';powershell='C:\fixture\pwsh.exe';version='2.44.0' }
    }
    function script:Initialize-SubscriptionDependency { throw 'Package installation forbidden in test fixture.' }
    function script:Assert-SubscriptionConfiguration {}
    function script:Get-SubscriptionTask([string]$Name) {
        if ($script:taskSamples.Count) { return $script:taskSamples.Dequeue() }
        $script:tasks[$Name]
    }
    function script:New-SubscriptionTaskXml($Paths,$PowerShell) { $script:descriptors[$Paths.task] = $Paths; '<Task>' + $Paths.task + '</Task>' }
    function script:Set-SubscriptionTask([string]$Name,$Xml,$ExpectedXml,$Paths) {
        $script:taskSetCalls++
        $current = $script:tasks[$Name]
        if (($current -and $current.xml -cne $ExpectedXml) -or (-not $current -and $ExpectedXml)) { throw 'Fixture task changed.' }
        if ($Xml) { $script:tasks[$Name] = @{ xml=$Xml;running=$false } } else { $script:tasks.Remove($Name) }
    }
    function script:Start-SubscriptionTask([string]$Name) {
        $script:taskStartCalls++
        $script:tasks[$Name].running = $true
        $paths = $script:descriptors[$Name]
        $text = [Text.Encoding]::UTF8.GetString((Get-CodeToolsBytes $paths.config))
        if (-not $text.StartsWith('# fixture route')) { Write-CodeToolsBytes $paths.config ([Text.Encoding]::UTF8.GetBytes("# fixture route`r`n" + $text)) }
        $script:started[$paths.codex] = @{processId=42;assignedBeforeResume=$true}
    }
    function script:Get-SubscriptionStarted($Paths) {
        if ($script:useActualReceipt) { & $script:realStarted $Paths } else { $script:started[$Paths.codex] }
    }
    function script:Get-SubscriptionOwnedProcess($Paths) {
        if ($script:useStopFixture) { $script:stopProcess } else { & $script:realOwnedProcess $Paths }
    }
    function script:Restore-SubscriptionNative($Paths,$Dependency) {
        $script:nativeRestoreCalls++
        if ($script:failCleanup) { throw 'fixture cleanup conflict' }
        $text = [Text.Encoding]::UTF8.GetString((Get-CodeToolsBytes $Paths.config))
        Write-CodeToolsBytes $Paths.config ([Text.Encoding]::UTF8.GetBytes($text.Replace("# fixture route`r`n",'')))
    }
    function script:Test-SubscriptionReady { -not $script:failReady }
    function script:Wait-SubscriptionReady { if ($script:failReady) { throw 'Fixture readiness failed.' } }
}
try {
    $fixture = New-Fixture preview
    $before = Snapshot $fixture
    & $module { $script:missingPackage = $true }
    Invoke-HarnessSubscriptionRouting @fixture -Mode Install -Preview | Out-Null
    Assert-True ((Invoke-HarnessSubscriptionRouting @fixture -Mode Check).status -eq 'disconnected') 'Check incorrectly claimed readiness without a package.'
    Assert-Snapshot $fixture $before
    Assert-True (-not (Test-Path (Join-Path $fixture.CodexHome 'harness'))) 'Preview/Check created host state.'
    & $module { $script:missingPackage = $false }
    Write-Output 'PASS preview/check neither install package nor write state'

    $fixture = New-Fixture lifecycle; $native = Snapshot $fixture
    Invoke-HarnessSubscriptionRouting @fixture -Mode Install | Out-Null
    $connected = Snapshot $fixture
    Assert-True ((Invoke-HarnessSubscriptionRouting @fixture -Mode Check).status -eq 'ready') 'Installed fixture not ready.'
    foreach ($link in @((Join-Path $fixture.UserHome '.opencodex/config.json'),(Join-Path $fixture.CodexHome 'agents/codex-harness-subscriptions'))) { Assert-True ((Get-Item -LiteralPath $link).LinkType -eq 'SymbolicLink') 'Source connection was copied.' }
    Invoke-HarnessSubscriptionRouting @fixture -Mode Install | Out-Null
    Assert-Snapshot $fixture $connected
    Invoke-HarnessSubscriptionRouting @fixture -Mode Disconnect | Out-Null
    Assert-Snapshot $fixture $native
    Assert-True (-not (Test-Path (Join-Path $fixture.CodexHome 'agents/codex-harness-subscriptions'))) 'Role remained active after disconnect.'
    Assert-True ((Read-CodeToolsJson (Join-Path $fixture.UserHome '.opencodex/auth.json')).fixture -eq 'private-auth-preserved') 'Authentication was changed.'
    Write-Output 'PASS install/repeat/disconnect preserve exact native bytes and auth; source links are direct'

    $failed = New-Fixture startupFailure; $before = Snapshot $failed
    & $module { $script:failReady = $true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @failed -Mode Install } 'readiness failed'
    & $module { $script:failReady = $false }
    Assert-Snapshot $failed $before
    Assert-True (-not (Test-Path (Join-Path $failed.CodexHome 'harness/subscription-routing-pending.json'))) 'Startup failure did not recover.'
    Write-Output 'PASS startup failure restores prior bytes/absence and removes owned task/links'

    Invoke-HarnessSubscriptionRouting @fixture -Mode Install | Out-Null
    $before = Snapshot $fixture
    foreach ($mode in @('Update','Disconnect')) {
        Assert-Throw { Invoke-HarnessSubscriptionRouting @fixture -Mode $mode -Checkpoint { throw 'injected late failure' } } 'injected late failure'
        Assert-Snapshot $fixture $before
    }
    Write-Output 'PASS late update/disconnect failure restores routed prior state'

    Assert-Throw { Invoke-HarnessSubscriptionRouting @fixture -Mode Update -DeferCommit -Checkpoint { throw 'outer failure fixture' } } 'coordinator recovery required'
    Restore-HarnessSubscriptionRouting @fixture -DeferRestart | Out-Null
    $paths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $fixture
    Assert-True (-not (& $module { param($p) (Get-SubscriptionTask $p.task).running } $paths)) 'Routing writer restarted before outer recovery.'
    Assert-True ((Read-CodeToolsJson $paths.pending).phase -eq 'recovered') 'Deferred restart did not persist completed file recovery.'
    Add-Content -LiteralPath $paths.config '# outer MCP restore fixture'
    $outerBytes = Get-CodeToolsBytes $paths.config
    Restore-HarnessSubscriptionRouting @fixture -DeferRestart | Out-Null
    Assert-True ((Get-CodeToolsHash (Get-CodeToolsBytes $paths.config)) -eq (Get-CodeToolsHash $outerBytes)) 'Repeated recovery overwrote the completed MCP recovery phase.'
    Resume-HarnessSubscriptionRouting -SourceRoot $fixture.SourceRoot -UserHome $fixture.UserHome -CodexHome $fixture.CodexHome
    Assert-True (-not (Test-Path -LiteralPath $paths.pending)) 'Deferred restart did not complete cleanup.'
    $before = Snapshot $fixture
    Write-Output 'PASS outer recovery defers restart; interrupted resume preserves completed MCP recovery'

    Assert-Throw { Invoke-HarnessSubscriptionRouting @fixture -Mode Update -Checkpoint {
        Add-Content -LiteralPath (Join-Path $fixture.CodexHome 'config.toml') '# concurrent foreign edit'
        throw 'injected conflict'
    } } 'Recovery pending'
    $configPath = Join-Path $fixture.CodexHome 'config.toml'
    Assert-True ((Get-Content -LiteralPath $configPath -Raw).Contains('concurrent foreign edit')) 'Recovery overwrote a foreign edit.'
    $pending = Read-CodeToolsJson (Join-Path $fixture.CodexHome 'harness/subscription-routing-pending.json')
    Write-CodeToolsBytes $configPath ([Convert]::FromBase64String($pending.config_native))
    Invoke-HarnessSubscriptionRouting @fixture -Mode Recover | Out-Null
    Assert-Snapshot $fixture $before
    Write-Output 'PASS concurrent config preserved; explicit resolution allows repeated recovery'

    $restartFixture = New-Fixture restart
    Invoke-HarnessSubscriptionRouting @restartFixture -Mode Install | Out-Null
    $restartPaths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $restartFixture
    Remove-Item -LiteralPath $restartPaths.roleLink -Force
    $monitor = @{ready=$false;deadline=[DateTime]::UtcNow.AddSeconds(30)}
    & $module { param($p,$m) $script:failReady=$true; Update-SubscriptionServiceReadiness $p 10100 $m } $restartPaths $monitor
    Assert-True (-not $monitor.ready -and -not (Test-Path -LiteralPath $restartPaths.roleLink)) 'Restart activated a role before readiness.'
    & $module { param($p,$m) $script:failReady=$false; Update-SubscriptionServiceReadiness $p 10100 $m } $restartPaths $monitor
    Assert-True ($monitor.ready -and (Get-Item -LiteralPath $restartPaths.roleLink).LinkTarget -eq $restartPaths.roleSource) 'Ready restart did not reconnect its owned role.'
    Remove-Item -LiteralPath $restartPaths.roleLink -Force
    Write-CodeToolsJson $restartPaths.pending @{fixture='installer owns activation'}
    & $module { param($p) Update-SubscriptionServiceReadiness $p 10100 @{ready=$false;deadline=[DateTime]::UtcNow.AddSeconds(30)} } $restartPaths
    Assert-True (-not (Test-Path -LiteralPath $restartPaths.roleLink)) 'Service raced the installer journal link writer.'
    Remove-CodeToolsFile $restartPaths.pending
    Write-CodeToolsBytes $restartPaths.roleLink ([Text.Encoding]::UTF8.GetBytes('foreign role'))
    Assert-Throw { & $module { param($p) Update-SubscriptionServiceReadiness $p 10100 @{ready=$false;deadline=[DateTime]::UtcNow.AddSeconds(30)} } $restartPaths } 'Foreign connection'
    Assert-True ((Get-Content -LiteralPath $restartPaths.roleLink -Raw) -eq 'foreign role') 'Service restart overwrote a foreign role.'
    Remove-CodeToolsFile $restartPaths.roleLink
    & $module { $script:failReady=$true }
    Assert-Throw { & $module { param($p) Update-SubscriptionServiceReadiness $p 10100 @{ready=$false;deadline=[DateTime]::UtcNow.AddSeconds(-1)} } $restartPaths } 'within 90 seconds'
    & $module {
        $script:failReady=$false; $script:observedServiceReady=$false
        $script:failPort = $false
        function script:Assert-SubscriptionPortFree { if ($script:failPort) { throw 'fixture port occupied by another listener' } }
        function script:Invoke-SubscriptionBounded {
            param($Paths,$Executable,$Arguments,$Environment,$Timeout,[switch]$ServiceRun,$OnRunning)
            if ($Executable -ne 'C:\fixture\bun.exe' -or ($Arguments -join '|') -ne '--no-env-file|C:\fixture\index.ts|start|--port|10100' -or
                $Environment.CODEX_HOME -ne $Paths.codex -or $Environment.OPENCODEX_HOME -ne $Paths.opencodex -or
                $Environment.OCX_SERVICE -ne '1' -or $Timeout -ne 0) { throw 'Unexpected service command or isolation parameters.' }
            $script:boundedServiceCalls++
            if (-not $ServiceRun) { throw 'Unexpected bounded command in service fixture.' }
            & $OnRunning
            $script:observedServiceReady = (Get-SubscriptionLink $Paths.roleLink) -eq $Paths.roleSource
            throw 'fixture service exit'
        }
    }
    foreach ($attempt in 1..2) {
        $runtimeFailure=$null
        try { Invoke-SubscriptionServiceHost -StatePath $restartPaths.service } catch { $runtimeFailure=$_ }
        Assert-True ($runtimeFailure -and $runtimeFailure.Exception.Message -match 'fixture service exit' -and $runtimeFailure.Exception.Data['SubscriptionRuntimeRetryable'] -eq $true) 'Runtime failure lost its original error or retry marker.'
        Assert-True (& $module { $script:observedServiceReady }) 'Service entry did not observe ready role before exit.'
        Assert-True (-not (Test-Path -LiteralPath $restartPaths.roleLink)) 'Service exit left its role active after native restore.'
    }
    & $module { $script:failCleanup=$true }
    try {
        $cleanupFailure=$null
        try { Invoke-SubscriptionServiceHost -StatePath $restartPaths.service } catch { $cleanupFailure=$_ }
        Assert-True ($cleanupFailure -and $cleanupFailure.Exception.Message -match 'fixture cleanup conflict' -and $cleanupFailure.Exception.Data['SubscriptionRuntimeRetryable'] -ne $true) 'Cleanup conflict incorrectly retained runtime retry permission.'
    } finally { & $module { $script:failCleanup=$false } }
    Write-Output 'PASS ready service restart reconnects roles; pending/foreign role/timeout and exit cleanup protected'

    $portFixture = New-Fixture portOccupied
    $portNativeBefore = [Convert]::ToBase64String((Get-CodeToolsBytes (Join-Path $portFixture.CodexHome 'config.toml')))
    Invoke-HarnessSubscriptionRouting @portFixture -Mode Install | Out-Null
    $portPaths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $portFixture
    & $module { $script:failPort=$true; $script:nativeRestoreCalls=0; $script:boundedServiceCalls=0 }
    try {
        Assert-Throw { Invoke-SubscriptionServiceHost -StatePath $portPaths.service } 'fixture port occupied'
        Assert-True (& $module { $script:nativeRestoreCalls -eq 1 -and $script:boundedServiceCalls -eq 0 }) 'Busy-port failure did not restore owned routing before exiting without a runtime launch.'
        Assert-True (-not (Test-Path -LiteralPath $portPaths.roleLink)) 'Busy-port failure left the Grok role active.'
        Assert-True ([Convert]::ToBase64String((Get-CodeToolsBytes $portPaths.config)) -ceq $portNativeBefore) 'Busy-port failure did not restore exact native configuration bytes.'
    } finally { & $module { $script:failPort=$false } }
    Write-Output 'PASS occupied port restores prior owned native routing and removes role without launching a runtime'

    foreach ($replacement in @('missing','file','different-link')) {
        $guardFixture = New-Fixture ('serviceConfig-' + $replacement)
        Invoke-HarnessSubscriptionRouting @guardFixture -Mode Install | Out-Null
        $guardPaths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $guardFixture
        Remove-Item -LiteralPath $guardPaths.configLink -Force
        $foreignTarget = Join-Path $guardFixture.SourceRoot 'foreign-config.json'
        if ($replacement -eq 'file') { Write-CodeToolsJson $guardPaths.configLink @{foreign='preserve-config-bytes'} }
        if ($replacement -eq 'different-link') {
            Write-CodeToolsJson $foreignTarget @{foreign='preserve-target-bytes'}
            New-Item -ItemType SymbolicLink -Path $guardPaths.configLink -Target $foreignTarget | Out-Null
        }
        $guardBefore = Snapshot $guardFixture
        $sourceBefore = Get-FileHash -LiteralPath $guardPaths.configSource
        $foreignBefore = if ($replacement -ne 'missing') { Get-FileHash -LiteralPath $guardPaths.configLink } else { $null }
        & $module { $script:nativeRestoreCalls = 0; $script:boundedServiceCalls = 0 }
        Assert-Throw { Invoke-SubscriptionServiceHost -StatePath $guardPaths.service } 'Foreign connection|configuration link is missing or changed'
        Assert-True (& $module { $script:boundedServiceCalls -eq 0 }) 'Changed service config reached bounded process launch.'
        Assert-True (& $module { $script:nativeRestoreCalls -eq 0 }) 'Changed service config triggered native restoration.'
        Assert-Snapshot $guardFixture $guardBefore
        Assert-True ((Get-FileHash -LiteralPath $guardPaths.configSource).Hash -eq $sourceBefore.Hash) 'Rejected service start changed repository config.'
        Assert-True ((Get-Item -LiteralPath $guardPaths.roleLink).LinkTarget -eq $guardPaths.roleSource) 'Rejected service start removed the existing role.'
        if ($replacement -eq 'missing') {
            Assert-True (-not (Test-Path -LiteralPath $guardPaths.configLink)) 'Rejected service start recreated missing config.'
        } else {
            Assert-True ((Get-FileHash -LiteralPath $guardPaths.configLink).Hash -eq $foreignBefore.Hash) 'Rejected service start changed foreign configuration bytes.'
            $linkTarget = (Get-Item -LiteralPath $guardPaths.configLink).LinkTarget
            Assert-True $(if ($replacement -eq 'file') { -not $linkTarget } else { $linkTarget -eq $foreignTarget }) 'Rejected service start replaced foreign connection.'
        }
    }
    Write-Output 'PASS independent service starts reject missing, foreign file and different config links before launch or cleanup'

    $markerFixture = New-Fixture stoppedMarkers
    $markerPaths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $markerFixture
    $markerRuntime = Join-Path $markerPaths.opencodex 'runtime-port.json'
    $markerPidPath = Join-Path $markerPaths.opencodex 'ocx.pid'
    $markerReceipt = Join-Path $markerPaths.runtime 'previous.started.json'
    $markerActive = Join-Path $markerPaths.runtime 'active-run.json'
    $deadPid = 2147483000
    Assert-True (& $module { param($id) -not (Test-SubscriptionProcessAlive $id) } $deadPid) 'Chosen isolated dead PID is live.'
    Assert-True (& $module { param($id) Test-SubscriptionProcessAlive $id } $PID) 'Liveness check missed the running test process.'
    & $module { $script:useActualReceipt = $true }
    try {
        foreach ($scenario in @('dead-owned','missing-receipt','foreign-runtime','live-owned','foreign-pid')) {
            $receiptPid = if ($scenario -eq 'live-owned') { $PID } else { $deadPid }
            Write-CodeToolsJson $markerReceipt @{processId=$receiptPid;assignedBeforeResume=$true}
            Write-CodeToolsJson $markerActive @{started=$markerReceipt}
            Write-CodeToolsJson $markerRuntime @{pid=$(if ($scenario -eq 'foreign-runtime') { $PID } else { $receiptPid });port=10100}
            Write-CodeToolsBytes $markerPidPath ([Text.Encoding]::UTF8.GetBytes([string]$(if ($scenario -eq 'foreign-pid') { $PID } else { $receiptPid })))
            if ($scenario -eq 'missing-receipt') { Remove-CodeToolsFile $markerActive }
            $runtimeBefore = [Convert]::ToBase64String((Get-CodeToolsBytes $markerRuntime))
            $pidBefore = [Convert]::ToBase64String((Get-CodeToolsBytes $markerPidPath))
            if ($scenario -eq 'dead-owned') {
                & $module { param($p) Remove-SubscriptionStoppedMarker $p; Remove-SubscriptionStoppedMarker $p } $markerPaths
                Assert-True (-not (Test-Path -LiteralPath $markerRuntime) -and -not (Test-Path -LiteralPath $markerPidPath)) 'Dead owned markers survived repeated cleanup.'
                Assert-True ((Read-CodeToolsJson $markerActive).started -eq $markerReceipt) 'Marker cleanup discarded its current ownership receipt.'
            } else {
                Assert-Throw { & $module { param($p) Remove-SubscriptionStoppedMarker $p } $markerPaths } 'preserving'
                Assert-True ([Convert]::ToBase64String((Get-CodeToolsBytes $markerRuntime)) -ceq $runtimeBefore) 'Rejected cleanup changed runtime marker.'
                Assert-True ([Convert]::ToBase64String((Get-CodeToolsBytes $markerPidPath)) -ceq $pidBefore) 'Rejected cleanup changed PID marker.'
            }
        }
        Write-CodeToolsJson $markerReceipt @{processId=$deadPid;assignedBeforeResume=$true}
        Write-CodeToolsJson $markerActive @{started=$markerReceipt}
        Write-CodeToolsJson $markerRuntime @{pid=$deadPid;port=10100}
        Write-CodeToolsBytes $markerPidPath ([Text.Encoding]::UTF8.GetBytes([string]$deadPid))
        $fakeRunner = @'
param($RequestPath,$ResultPath,[switch]$PassThru,$OnRunning)
$request = Get-Content -LiteralPath $RequestPath -Raw | ConvertFrom-Json
foreach ($name in @('runtime-port.json','ocx.pid')) {
    if (Test-Path -LiteralPath (Join-Path $request.environment.OPENCODEX_HOME $name)) { throw 'Stale marker reached new service launch.' }
}
@{ExitCode=0;Status='fixture'}
'@
        Write-CodeToolsBytes (Join-Path $markerFixture.SourceRoot 'tools/opencodex-process.ps1') ([Text.Encoding]::UTF8.GetBytes($fakeRunner))
        & $module { param($p) & $script:realBounded $p 'C:\fixture\never-executed.exe' @() @{OPENCODEX_HOME=$p.opencodex} -Timeout 0 -ServiceRun | Out-Null } $markerPaths
        Assert-True ((Read-CodeToolsJson $markerActive).started -ne $markerReceipt) 'New service did not replace the retired run receipt.'
        Assert-True (-not (Test-Path -LiteralPath $markerRuntime) -and -not (Test-Path -LiteralPath $markerPidPath)) 'New service launched before old marker retirement.'
    } finally { & $module { $script:useActualReceipt = $false } }
    Write-Output 'PASS exact dead-owned marker retirement before receipt replacement; live, foreign and unproven markers preserved'

    & $module { $script:useStopFixture = $true }
    try {
        foreach ($exitsAfterStop in @($true,$false)) {
            $stopEvents = [Collections.Generic.List[string]]::new()
            $stopProcess = [pscustomobject]@{events=$stopEvents;exits=$exitsAfterStop;waitMilliseconds=0;disposed=$false}
            $stopProcess | Add-Member ScriptMethod WaitForExit {
                param($milliseconds)
                $this.events.Add('wait'); $this.waitMilliseconds = $milliseconds
                if ($this.exits) { Start-Sleep -Milliseconds 20 }
                return $this.exits
            }
            $stopProcess | Add-Member ScriptMethod Dispose { $this.events.Add('dispose'); $this.disposed=$true }
            $stopTask = [pscustomobject]@{events=$stopEvents;State=4}
            $stopTask | Add-Member ScriptMethod Stop { param($flags) if ($flags -ne 0) { throw 'Unexpected task stop flags.' }; $this.events.Add('stop'); $this.State=3 }
            & $module { param($process) $script:stopProcess=$process } $stopProcess
            if ($exitsAfterStop) { & $module { param($task,$p) Stop-SubscriptionTaskRuntime $task $p } $stopTask $markerPaths }
            else { Assert-Throw { & $module { param($task,$p) Stop-SubscriptionTaskRuntime $task $p } $stopTask $markerPaths } 'runtime did not stop within 10 seconds' }
            Assert-True (($stopEvents -join ',') -eq 'stop,wait,dispose') 'Task stop did not wait for its runtime before disposing the process handle.'
            Assert-True ($stopProcess.waitMilliseconds -gt 0 -and $stopProcess.waitMilliseconds -le 10000) 'Runtime stop exceeded the shared 10-second deadline.'
            Assert-True $stopProcess.disposed 'Runtime stop leaked its process handle.'
        }
    } finally { & $module { $script:useStopFixture = $false } }
    Write-Output 'PASS owned task stop waits for delayed runtime exit and refuses a runtime that remains alive'

    Write-CodeToolsJson (Join-Path $paths.opencodex 'runtime-port.json') @{pid=999;port=10101}
    Assert-Throw { & $module { param($p) & $script:realNativeRestore $p @{bun='must-not-execute.exe';root='foreign'} } $paths } 'Another OpenCodex runtime'
    Write-CodeToolsJson (Join-Path $paths.opencodex 'runtime-port.json') @{pid=42;port=10100}
    $readyChecks = & $module {
        param($p)
        function script:Invoke-RestMethod { param($Uri,$TimeoutSec,[switch]$NoProxy,[switch]$DisableKeepAlive) if ($Uri -ne 'http://127.0.0.1:10100/readyz' -or -not $NoProxy -or -not $DisableKeepAlive -or $TimeoutSec -ne 2) { throw 'Wrong loopback readiness transport.' }; $script:readyResponse }
        $script:readyResponse = @{status='ready';service='opencodex';pid=999;port=10100}
        $mismatched = & $script:realReady $p 10100
        $script:readyResponse.pid = 42
        $owned = & $script:realReady $p 10100
        $script:readyResponse.status = 'pending'
        $pending = & $script:realReady $p 10100
        @{ mismatched=$mismatched; owned=$owned; pending=$pending }
    } $paths
    Assert-True (-not $readyChecks.mismatched -and $readyChecks.owned -and -not $readyChecks.pending) 'Readiness did not attest live service PID/port/status.'
    Remove-CodeToolsFile (Join-Path $paths.opencodex 'runtime-port.json')
    Write-Output 'PASS foreign runtime native-restore refusal and exact /readyz PID/port/status contract'

    & $module {
        $script:taskSamples.Enqueue(@{running=$false;task_state=2;last_result=0x41303;last_run=[DateTime]::UtcNow.AddDays(-1).ToString('o')})
        $script:taskSamples.Enqueue(@{running=$false;task_state=3;last_result=1;last_run=[DateTime]::UtcNow.ToString('o')})
    }
    Assert-Throw { & $module { param($p) & $script:realWaitReady $p 10100 } $paths } 'lastResult=0x00000001'
    Assert-True (& $module { $script:taskSamples.Count -eq 0 }) 'Queued task was misclassified as a fresh completed failure.'
    Write-Output 'PASS scheduler failure retains fresh result code and permits queued startup'

    $restartWait = & $module {
        param($p)
        $savedReady = (Get-Command Test-SubscriptionReady).ScriptBlock
        $script:restartReadyCalls = 0
        function script:Test-SubscriptionReady {
            param($Paths, $Port)
            if ($Port -ne 10100) { throw 'Unexpected readiness fixture port.' }
            $script:restartReadyCalls++
            if ($script:restartReadyCalls -eq 2) { Set-SubscriptionLink $Paths.roleLink $Paths.roleSource $null }
            return $true
        }
        try {
            if (Get-SubscriptionLink $p.roleLink) { Set-SubscriptionLink $p.roleLink $null $p.roleSource }
            $script:tasks[$p.task].running = $true
            & $script:realWaitReady $p 10100
            $completedCalls = $script:restartReadyCalls
            Set-SubscriptionLink $p.roleLink $null $p.roleSource
            Write-CodeToolsJson $p.pending @{fixture='installer owns role'}
            $script:restartReadyCalls = 0
            & $script:realWaitReady $p 10100
            @{restartCalls=$completedCalls;installCalls=$script:restartReadyCalls;installerRole=(Get-SubscriptionLink $p.roleLink)}
        } finally {
            Set-Item -LiteralPath Function:script:Test-SubscriptionReady -Value $savedReady
            Remove-CodeToolsFile $p.pending
        }
    } $restartPaths
    Assert-True ($restartWait.restartCalls -eq 2 -and $restartWait.installCalls -eq 1 -and -not $restartWait.installerRole) 'Restart returned before the exact role link; or Install waited on its own future link write.'
    Write-Output 'PASS restart waits for the exact role link while pending installation owns its subsequent link write'

    $serviceEntry = New-Fixture serviceEntry
    $entryDescriptor = Join-Path $serviceEntry.CodexHome 'harness/subscriptions/service.json'
    Write-CodeToolsJson $entryDescriptor @{fixture='never-log-this-secret'}
    foreach ($attempt in 1..2) {
        Assert-Throw { & (Join-Path $repository 'tools/opencodex-service.ps1') -StatePath $entryDescriptor } 'private entry record'
    }
    $entryLogs = @(Get-ChildItem -LiteralPath (Join-Path (Split-Path $entryDescriptor) 'runs') -Filter '*.host.jsonl')
    Assert-True ($entryLogs.Count -eq 2) 'Repeated early service failures overwrote entry logs.'
    foreach ($entryLog in $entryLogs) {
        $entryText = Get-Content -LiteralPath $entryLog.FullName -Raw
        $entryRecords = @(Get-Content -LiteralPath $entryLog.FullName | ConvertFrom-Json)
        Assert-True ($entryRecords[0].stage -eq 'entry' -and $entryRecords[-1].stage -eq 'failed:service-host') 'Service entry failed before capturing its stage.'
        Assert-True (-not $entryText.Contains('never-log-this-secret') -and [bool]$entryRecords[-1].exceptionType) 'Service entry log exposed descriptor data or lost error type.'
    }
    $scheduledHost = & $module { Resolve-SubscriptionPowerShell }
    Assert-True ($scheduledHost -notmatch '(?i)\\WindowsApps\\' -and (Test-Path -LiteralPath $scheduledHost)) 'Scheduler did not select an installed native PowerShell.'
    Write-Output 'PASS real service entry logs early errors without descriptor data; native scheduled host selected'

    $foreign = New-Fixture foreign
    $foreignPath = Join-Path $foreign.UserHome '.opencodex/config.json'
    Write-CodeToolsJson $foreignPath @{ foreign = $true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @foreign -Mode Install -Preview } 'Foreign connection'
    Assert-True ((Read-CodeToolsJson $foreignPath).foreign) 'Foreign config was overwritten.'
    $paths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $fixture
    & $module { param($p) $script:tasks[$p.task].xml = '<Task>foreign</Task>' } $paths
    Assert-Throw { Invoke-HarnessSubscriptionRouting @fixture -Mode Disconnect } 'changed subscription task'
    Write-Output 'PASS foreign configuration and changed task refused without overwrite'
    $nativeTask = & $module {
        param($p)
        $missing = & $script:realTaskRead ('codex-harness-test-absent-' + [guid]::NewGuid().ToString('N'))
        $xml = & $script:realTaskXml $p (Get-Command pwsh -CommandType Application).Source
        @{ missing=$missing; xml=$xml }
    } $paths
    Assert-True (-not $nativeTask.missing) 'Missing Windows task did not produce an absent result.'
    $taskXml = [xml]$nativeTask.xml
    $restart = $taskXml.Task.Settings.SelectSingleNode('*[local-name()="RestartOnFailure"]')
    Assert-True ($restart -and $restart.SelectSingleNode('*[local-name()="Count"]').InnerText -eq '3' -and $restart.SelectSingleNode('*[local-name()="Interval"]').InnerText -eq 'PT1M') 'Task definition did not limit recovery to three attempts one minute apart.'
    Assert-True ($taskXml.Task.Actions.Exec.Arguments.Contains('-WindowStyle Hidden') -and $taskXml.Task.Settings.ExecutionTimeLimit -eq 'PT0S') 'Native task factory did not produce the hidden foreground action.'
    $currentIdentity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $expectedRunLevel = if ([Security.Principal.WindowsPrincipal]::new($currentIdentity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { 'HighestAvailable' } else { 'LeastPrivilege' }
        Assert-True ($taskXml.Task.Principals.Principal.RunLevel -eq $expectedRunLevel) 'Task privilege does not match the installer token.'
        Assert-True ($taskXml.Task.Principals.Principal.UserId -eq $currentIdentity.User.Value) 'Task principal changed the installer user identity.'
    } finally { $currentIdentity.Dispose() }
    Write-Output 'PASS actual Windows Task Scheduler read/definition APIs (no task registered)'
    # Real task-definition parsing, with only the task folder writes replaced.
    # Any unexpected Stop/Delete/Run call has no fixture method and fails.
    $policyFixture = New-Fixture restartPolicy
    Invoke-HarnessSubscriptionRouting @policyFixture -Mode Install | Out-Null
    $policyPaths = & $module { param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome } $policyFixture
    $policyLegacy = & $module {
        param($p)
        $xml = & $script:realTaskXml $p (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source
        $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect()
        $definition = $scheduler.NewTask(0); $definition.XmlText = $xml
        # A zero Count with the old Interval still present is invalid on reload.
        $legacyDocument = [xml]$definition.XmlText
        $legacyRestart = $legacyDocument.Task.Settings.SelectSingleNode('*[local-name()="RestartOnFailure"]')
        [void]$legacyRestart.ParentNode.RemoveChild($legacyRestart)
        $definition.XmlText = $legacyDocument.OuterXml
        $xml = $definition.XmlText
        $script:tasks[$p.task].xml = $xml
        $state = Read-CodeToolsJson $p.state; $state.task_xml = $xml; Write-CodeToolsJson $p.state $state
        $script:policyFolder = [pscustomobject]@{ Tasks=$script:tasks; Calls=0; FailRollback=$false; Legacy=$xml; ChangeTaskOnRead=$false }
        $script:policyFolder | Add-Member ScriptMethod GetTask {
            param($name)
            if ($this.ChangeTaskOnRead) { $this.ChangeTaskOnRead=$false; $this.Tasks[$name].xml='<Task>foreign at write</Task>' }
            [pscustomobject]@{ Xml=$this.Tasks[$name].xml }
        }
        function Invoke-FixtureTaskRegistration {
            [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingPlainTextForPassword', 'password', Justification = 'COM mock preserves RegisterTask arguments and rejects every non-null credential; no password is accepted or stored.')]
            [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSAvoidUsingUsernameAndPasswordParams', '', Justification = 'COM mock must retain the RegisterTask signature; assertions require null user and password arguments.')]
            param($name,$xml,$flags,$userId,$password,$logonType,$sddl)
            if ($flags -ne 36 -or $null -ne $userId -or $null -ne $password -or $logonType -ne 3 -or $null -ne $sddl) { throw 'Unexpected live task update effects.' }
            $this.Calls++
            if ($this.FailRollback -and $xml -ceq $this.Legacy) { $this.FailRollback=$false; throw 'fixture rollback interrupted' }
            $this.Tasks[$name].xml=$xml
        }
        $script:policyFolder | Add-Member ScriptMethod RegisterTask ${function:Invoke-FixtureTaskRegistration}
        function script:Get-SubscriptionTaskFolder { $script:policyFolder }
        $script:realPolicyWriter = (Get-Command Write-CodeToolsBytes).ScriptBlock
        $script:policyStatePath = $p.state; $script:failPolicyStateWrite=$false; $script:foreignPolicyState=$false
        function script:Write-CodeToolsBytes {
            [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Mock name must match the production command being overridden.')]
            param([string]$Path, [byte[]]$Bytes)
            if ($Path -eq $script:policyStatePath -and $script:failPolicyStateWrite) {
                $script:failPolicyStateWrite=$false
                if ($script:foreignPolicyState) { & $script:realPolicyWriter $Path ([Text.Encoding]::UTF8.GetBytes('{"foreign":true}')) }
                throw 'fixture policy state write failed'
            }
            & $script:realPolicyWriter $Path $Bytes
        }
        $xml
    } $policyPaths
    $policyBefore = Snapshot $policyFixture
    $policyAuth = Get-CodeToolsHash (Get-CodeToolsBytes (Join-Path $policyFixture.UserHome '.opencodex/auth.json'))
    $policyRuntime = & $module { @($script:taskSetCalls,$script:taskStartCalls,$script:nativeRestoreCalls,$script:boundedServiceCalls) -join ',' }
    $policyPreview = Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart -Preview
    Assert-True ($policyPreview.changed -and $policyPreview.restartCount -eq 3 -and $policyPreview.restartInterval -eq 'PT1M') 'Policy preview did not describe bounded recovery.'
    Assert-Snapshot $policyFixture $policyBefore
    Assert-True ((& $module { $script:policyFolder.Calls }) -eq 0 -and -not (Test-Path $policyPaths.restartPending)) 'Policy preview wrote task or journal.'
    $policyResult = Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart
    $policyAfter = Snapshot $policyFixture
    Assert-True ($policyResult.changed -and (Invoke-HarnessSubscriptionRouting @policyFixture -Mode Check).status -eq 'ready') 'Configured policy broke committed task ownership or readiness.'
    $policyState = Read-CodeToolsJson $policyPaths.state
    $policyOriginalXml = [xml]$policyLegacy; $policyNewXml = [xml]$policyState.task_xml
    foreach ($document in @($policyOriginalXml,$policyNewXml)) {
        $node=$document.Task.Settings.SelectSingleNode('*[local-name()="RestartOnFailure"]')
        if ($node) { [void]$node.ParentNode.RemoveChild($node) }
    }
    Assert-True ($policyOriginalXml.OuterXml -ceq $policyNewXml.OuterXml) 'Policy update changed unrelated task settings.'
    Assert-True (-not (Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart).changed -and (& $module { $script:policyFolder.Calls }) -eq 1) 'Repeated policy update was not idempotent.'
    Assert-Snapshot $policyFixture $policyAfter
    foreach ($name in @('config.toml','harness/subscriptions/service.json')) { Assert-True ($policyBefore[$name] -ceq $policyAfter[$name]) 'Policy update changed configuration or service descriptor.' }
    Assert-True ($policyAuth -ceq (Get-CodeToolsHash (Get-CodeToolsBytes (Join-Path $policyFixture.UserHome '.opencodex/auth.json')))) 'Policy update changed authentication.'
    foreach ($name in @('configLink','roleLink')) { Assert-True ((& $module { param($path) Get-SubscriptionLink $path } $policyPaths[$name]) -eq $policyState.links[$name]) 'Policy update changed a source link.' }
    foreach ($pendingName in @('activation-pending.json','pending.json','subscription-routing-pending.json','code-tools-files-pending.json')) {
        $pendingPath=Join-Path $policyPaths.codex ('harness/' + $pendingName)
        Write-CodeToolsJson $pendingPath @{fixture='unfinished'}
        Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'unfinished harness operation'
        Remove-CodeToolsFile $pendingPath
    }
    # Return only this owned fixture to its recorded legacy state for failures.
    & $module { param($p,$xml) $script:tasks[$p.task].xml=$xml } $policyPaths $policyLegacy
    Write-CodeToolsBytes $policyPaths.state ([Convert]::FromBase64String($policyBefore['harness/subscription-routing.json']))
    & $module { $script:failPolicyStateWrite=$true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'prior policy restored.*fixture policy state write failed'
    Assert-Snapshot $policyFixture $policyBefore
    Assert-True ((& $module {param($p) $script:tasks[$p.task].xml} $policyPaths) -ceq $policyLegacy -and -not (Test-Path $policyPaths.restartPending)) 'Failed policy write did not restore exact task/journal state.'
    & $module { $script:failPolicyStateWrite=$true; $script:policyFolder.FailRollback=$true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'Recovery pending.*fixture rollback interrupted'
    Assert-True (Test-Path $policyPaths.restartPending) 'Interrupted rollback lost its journal.'
    foreach ($mode in @('Install','Update','Check','Disconnect','ConfigureRestart')) { Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode $mode } 'restart policy update needs' }
    $policyInterruptedTask = & $module {param($p) $script:tasks[$p.task].xml} $policyPaths
    Assert-True ((Invoke-HarnessSubscriptionRouting @policyFixture -Mode Recover -Preview).status -eq 'preview-subscription-restart-policy-recovery') 'Policy recovery preview was not routed.'
    Assert-True ((& $module {param($p) $script:tasks[$p.task].xml} $policyPaths) -ceq $policyInterruptedTask -and (Test-Path $policyPaths.restartPending)) 'Policy recovery preview mutated task or journal.'
    Assert-True ((Invoke-HarnessSubscriptionRouting @policyFixture -Mode Recover).status -eq 'subscriptions-restart-policy-recovered') 'Interrupted policy rollback did not recover.'
    Assert-Snapshot $policyFixture $policyBefore
    & $module { $script:failPolicyStateWrite=$true; $script:foreignPolicyState=$true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'Recovery pending.*ownership state changed'
    Assert-True ((Read-CodeToolsJson $policyPaths.state).foreign -and (Test-Path $policyPaths.restartPending)) 'Policy recovery overwrote foreign state or removed evidence.'
    Write-CodeToolsBytes $policyPaths.state ([Convert]::FromBase64String($policyBefore['harness/subscription-routing.json']))
    Invoke-HarnessSubscriptionRouting @policyFixture -Mode Recover | Out-Null
    & $module { $script:policyFolder.ChangeTaskOnRead=$true }
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'Recovery pending.*task changed'
    Assert-True ((& $module {param($p) $script:tasks[$p.task].xml} $policyPaths) -ceq '<Task>foreign at write</Task>' -and (Test-Path $policyPaths.restartPending)) 'Policy writer overwrote a task changed immediately before COM update.'
    & $module {param($p,$xml) $script:tasks[$p.task].xml=$xml} $policyPaths $policyLegacy
    Invoke-HarnessSubscriptionRouting @policyFixture -Mode Recover | Out-Null
    $foreignPolicy = Read-CodeToolsJson $policyPaths.state; $foreignPolicy.source += '-foreign'; Write-CodeToolsJson $policyPaths.state $foreignPolicy
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'source changed'
    Write-CodeToolsBytes $policyPaths.state ([Convert]::FromBase64String($policyBefore['harness/subscription-routing.json']))
    & $module { param($p) $script:tasks[$p.task].xml='<Task>foreign</Task>' } $policyPaths
    Assert-Throw { Invoke-HarnessSubscriptionRouting @policyFixture -Mode ConfigureRestart } 'Foreign or changed subscription task'
    Assert-True ($policyRuntime -ceq (& $module { @($script:taskSetCalls,$script:taskStartCalls,$script:nativeRestoreCalls,$script:boundedServiceCalls) -join ',' })) 'Live policy update or recovery stopped, started or rewrote runtime routing.'
    Write-Output 'PASS live restart policy preview/idempotence, exact ownership, preservation, rollback and interrupted recovery without runtime mutations'
    # Capture the actual scheduler normalization: registration prunes defaults
    # and changes element order. This unique task is never run, even on logon.
    $nativePolicyFixture=New-Fixture nativePolicyXml
    Invoke-HarnessSubscriptionRouting @nativePolicyFixture -Mode Install | Out-Null
    $nativePolicyPaths=& $module {param($f) Get-SubscriptionPaths $f.SourceRoot $f.UserHome $f.CodexHome} $nativePolicyFixture
    $nativeScheduler=New-Object -ComObject 'Schedule.Service'; $nativeScheduler.Connect(); $nativeFolder=$nativeScheduler.GetFolder('\')
    $nativeDefinition=$nativeScheduler.NewTask(0)
    $nativeDefinition.XmlText=& $module {param($p,$exe) & $script:realTaskXml $p $exe} $nativePolicyPaths $scheduledHost
    $nativeDefinition.Triggers.Clear()
    $nativeDefinition.Actions.Item(1).Arguments='-NoLogo -NoProfile -WindowStyle Hidden -Command "exit 0"'
    [xml]$nativeLegacy=$nativeDefinition.XmlText
    $nativeRestart=$nativeLegacy.Task.Settings.SelectSingleNode('*[local-name()="RestartOnFailure"]')
    [void]$nativeRestart.ParentNode.RemoveChild($nativeRestart)
    $nativeBeforeXml=$null; $nativeCandidateXml=$null
    try {
        $nativeTask=$nativeFolder.RegisterTask($nativePolicyPaths.task,$nativeLegacy.OuterXml,34,$null,$null,3,$null)
        $nativeBeforeXml=$nativeTask.Xml
        $nativeState=Read-CodeToolsJson $nativePolicyPaths.state; $nativeState.task_xml=$nativeBeforeXml; Write-CodeToolsJson $nativePolicyPaths.state $nativeState
        $nativeBeforeState=[Convert]::ToBase64String((Get-CodeToolsBytes $nativePolicyPaths.state))
        $nativeCandidateXml=& $module {param($xml) Get-SubscriptionRestartTaskXml $xml} $nativeBeforeXml
        & $module {
            function script:Get-SubscriptionTask([string]$Name) { & $script:realTaskRead $Name }
            function script:Get-SubscriptionTaskFolder { & $script:realTaskFolder }
        }
        Assert-True ((Invoke-HarnessSubscriptionRouting @nativePolicyFixture -Mode ConfigureRestart).changed) 'Native policy update did not apply.'
        $nativeActual=$nativeFolder.GetTask($nativePolicyPaths.task)
        Assert-True ($nativeActual.Xml -cne $nativeCandidateXml -and (& $module {param($a,$b) Test-SubscriptionTaskXmlEquivalent $a $b} $nativeActual.Xml $nativeCandidateXml)) 'Native regression did not exercise registered XML normalization.'
        Assert-True ((Read-CodeToolsJson $nativePolicyPaths.state).task_xml -ceq $nativeActual.Xml) 'Committed policy state did not store actual scheduler XML.'
        Assert-True (-not (Invoke-HarnessSubscriptionRouting @nativePolicyFixture -Mode ConfigureRestart).changed) 'Native policy update is not idempotent.'
        foreach ($field in @('arguments','principal','count')) {
            [xml]$foreignXml=$nativeCandidateXml
            if ($field -eq 'arguments') { $foreignXml.Task.Actions.Exec.Arguments += ' foreign' }
            elseif ($field -eq 'principal') { $foreignXml.Task.Principals.Principal.RunLevel=if($foreignXml.Task.Principals.Principal.RunLevel -eq 'HighestAvailable'){'LeastPrivilege'}else{'HighestAvailable'} }
            else { $foreignXml.Task.Settings.RestartOnFailure.Count='4' }
            Assert-True (-not (& $module {param($a,$b) Test-SubscriptionTaskXmlEquivalent $a $b} $nativeActual.Xml $foreignXml.OuterXml)) "Native normalization accepted a material $field change."
        }
        # Simulate interruption after RegisterTask, before actual XML is journaled.
        Write-CodeToolsBytes $nativePolicyPaths.state ([Convert]::FromBase64String($nativeBeforeState))
        $nativeState.task_xml=$nativeCandidateXml
        $nativePending=@{schema_version=1;owner='codex-harness-subscriptions';operation='restart-policy';source=$nativePolicyPaths.source;user=$nativePolicyPaths.user;codex=$nativePolicyPaths.codex;task=$nativePolicyPaths.task
            task_before=$nativeBeforeXml;task_after=$nativeCandidateXml;state_before=$nativeBeforeState;state_after=[Convert]::ToBase64String([Text.UTF8Encoding]::new($false).GetBytes(($nativeState|ConvertTo-Json -Depth 60)))}
        Write-CodeToolsJson $nativePolicyPaths.restartPending $nativePending
        Assert-True ((Invoke-HarnessSubscriptionRouting @nativePolicyFixture -Mode Recover).status -eq 'subscriptions-restart-policy-recovered') 'Native normalized candidate did not recover after interruption.'
        $nativeActual=$nativeFolder.GetTask($nativePolicyPaths.task)
        Assert-True ($nativeActual.Xml -ceq $nativeBeforeXml -and [Convert]::ToBase64String((Get-CodeToolsBytes $nativePolicyPaths.state)) -ceq $nativeBeforeState -and -not(Test-Path $nativePolicyPaths.restartPending)) 'Native rollback lost exact prior task/state or journal cleanup.'
        Assert-True ($nativeActual.State -ne 4 -and $nativeActual.LastRunTime.Year -lt 2000) 'The native XML fixture unexpectedly ran.'
    } finally {
        if ($nativeBeforeXml) {
            $nativeCleanup=$nativeFolder.GetTask($nativePolicyPaths.task)
            $nativeOwned=$nativeCleanup.Xml -ceq $nativeBeforeXml -or ($nativeCandidateXml -and (& $module {param($a,$b) Test-SubscriptionTaskXmlEquivalent $a $b} $nativeCleanup.Xml $nativeCandidateXml))
            if (-not $nativeOwned -or $nativeCleanup.State -eq 4) { throw 'Native XML fixture changed or running; preserving its task.' }
            $nativeFolder.DeleteTask($nativePolicyPaths.task,0)
        }
    }
    Write-Output 'PASS actual never-run task XML normalization, committed bytes, idempotence, material-change rejection and interrupted recovery'
    $entryFixture = New-Fixture installerEntry
    $entryBefore = Snapshot $entryFixture
    $installer = Join-Path $repository 'install.ps1'
    $entryArgs = @{UserHome=$entryFixture.UserHome;CodexHome=$entryFixture.CodexHome;SubscriptionsOnly=$true;PathScope='Process'}
    Assert-Throw { & $installer @entryArgs -CoreOnly -Mode Check } 'mutually exclusive'
    $policyEntryArgs = @{UserHome=$entryFixture.UserHome;CodexHome=$entryFixture.CodexHome;PathScope='Process'}
    Assert-Throw { & $installer @policyEntryArgs -Mode ConfigureRestart } 'ConfigureRestart requires -SubscriptionsOnly'
    Assert-Throw { & $installer @policyEntryArgs -CoreOnly -Mode ConfigureRestart } 'ConfigureRestart requires -SubscriptionsOnly'
    $entryPlan = & $installer @entryArgs -Mode Install -WhatIf
    Assert-True ($entryPlan.status -eq 'preview-subscriptions') 'Actual installer did not route subscriptions-only preview.'
    $entryCheck = & $installer @entryArgs -Mode Check
    Assert-True ($entryCheck.status -eq 'disconnected') 'Actual installer did not route subscriptions-only Check.'
    Assert-Snapshot $entryFixture $entryBefore
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $entryFixture.CodexHome 'harness'))) 'Subscriptions-only preview/Check wrote host state.'
    $entryPolicyJournal = Join-Path $entryFixture.CodexHome 'harness/subscription-restart-policy-pending.json'
    Write-CodeToolsJson $entryPolicyJournal @{fixture='policy transaction'}
    foreach ($entryMode in @(@{Mode='Install'},@{Mode='Recover'},@{CoreOnly=$true;Mode='Check'},@{SubscriptionsOnly=$true;Mode='Check'},@{SubscriptionsOnly=$true;Mode='ConfigureRestart'})) {
        Assert-Throw { & $installer @policyEntryArgs @entryMode } 'interrupted restart-policy update requires'
    }
    Assert-Snapshot $entryFixture $entryBefore
    Assert-True ((Read-CodeToolsJson $entryPolicyJournal).fixture -eq 'policy transaction' -and -not (Test-Path (Join-Path $entryFixture.CodexHome 'harness/activation-pending.json'))) 'Installer policy guard changed state before dispatch.'
    Remove-CodeToolsFile $entryPolicyJournal
    Write-CodeToolsJson (Join-Path $entryFixture.CodexHome 'harness/activation-pending.json') @{fixture='outer transaction'}
    Assert-Throw { & $installer @entryArgs -Mode Recover } 'without -SubscriptionsOnly'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $entryFixture.CodexHome 'harness/subscriptions'))) 'Pending outer transaction did not block component execution.'
    Write-Output 'PASS actual install.ps1 -SubscriptionsOnly preview/Check, mutual exclusion and outer recovery guard'
    Write-Output "$assertions subscription lifecycle assertions passed; runtime was a fixture and the native XML task never ran."
} finally {
    # Remove links individually before any recursive cleanup; never traverse a
    # target from a link during fixture disposal.
    if (Test-Path -LiteralPath $suite) {
        foreach ($directory in Get-ChildItem -LiteralPath $suite -Directory) {
            foreach ($relative in @('user/.opencodex/config.json','codex/agents/codex-harness-subscriptions')) {
                $path = Join-Path $directory.FullName $relative
                $item = Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
                if ($item -and $item.LinkType -eq 'SymbolicLink') { Remove-Item -LiteralPath $path -Force }
            }
        }
        $full = [IO.Path]::GetFullPath($suite)
        if ((Split-Path $full) -ine [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') -or (Split-Path $full -Leaf) -notlike 'codex-subscription-routing-*') { throw 'Fixture cleanup scope mismatch.' }
        Remove-Item -LiteralPath $full -Recurse -Force
    }
}
