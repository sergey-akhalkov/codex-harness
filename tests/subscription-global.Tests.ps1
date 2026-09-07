#requires -Version 7.4
<#
Opt-in, serial acceptance against an already connected global installation.
Temporarily stops ONLY its recorded task, exercises Recover/Disconnect, then
reconnects. No model requests, other process termination, credential writes,
or recursive deletion. Do not run during another subscription consumer probe.
BaselinePath is a private JSON map of absolute filenames to pre-install SHA256.
NativeBaselinePath optionally supplies the exact native snapshot for this cycle
when intervening native clients have changed configuration since that hash map.
The report retains the historical hash comparison separately; it is not waived.
Reports and bounded native CLI outputs remain in a new directory outside the repo.
RunReadinessProbe only observes the installed service and invokes native read-only
commands. It never installs, stops, recovers or reconnects the proxy and can run
from Codex. A failure preserves the observed state for diagnosis.
#>
[CmdletBinding()]
param([switch]$RunLifecycleProbes, [switch]$RunReadinessProbe,
    [ValidateRange(120,600)][int]$ObserveSeconds = 120,
    [string]$BaselinePath, [string]$NativeBaselinePath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $RunLifecycleProbes -and -not $RunReadinessProbe) { 'SKIP: choose -RunReadinessProbe or -RunLifecycleProbes, with -BaselinePath <private pre-install hash map>.'; return }
if ($RunLifecycleProbes -and $RunReadinessProbe) { throw 'Choose exactly one probe mode.' }
if ($RunLifecycleProbes -and ($env:CODEX_THREAD_ID -or $env:CODEX_SESSION_ID)) {
    throw 'Global subscription lifecycle probes cannot run from a Codex session: stopping its proxy can strand the controlling session. Use the isolated probe, or run this test from an independent terminal after closing proxy-dependent Codex sessions.'
}
if (-not [IO.Path]::IsPathFullyQualified($BaselinePath)) { throw 'An absolute baseline path is required.' }
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$codexDirectory = if ($env:CODEX_HOME) { [IO.Path]::GetFullPath($env:CODEX_HOME) } else { Join-Path $env:USERPROFILE '.codex' }
$module = Import-Module (Join-Path $repo 'tools/subscription-routing.psm1') -Force -PassThru
$paths = & $module { param($r,$u,$c) Get-SubscriptionPaths $r $u $c } $repo $env:USERPROFILE $codexDirectory
$state = Get-Content -LiteralPath $paths.state -Raw | ConvertFrom-Json -AsHashtable
& $module { param($s,$p) Assert-SubscriptionState $s $p } $state $paths
if ($state.source -ne $repo) { throw 'This checkout does not own the connected integration.' }
$installer = Join-Path $repo 'install.ps1'
$baseline = Get-Content -LiteralPath $BaselinePath -Raw | ConvertFrom-Json -AsHashtable
$historicalNativeHash = $baseline[$paths.config.Replace('\','/')]
if (-not $historicalNativeHash) { $historicalNativeHash = $baseline[$paths.config] }
if ($NativeBaselinePath) {
    if (-not [IO.Path]::IsPathFullyQualified($NativeBaselinePath)) { throw 'Native baseline must be an absolute snapshot path.' }
    $nativeHash = (Get-FileHash -LiteralPath $NativeBaselinePath).Hash
    foreach ($path in @($baseline.Keys)) {
        if ([IO.Path]::GetFullPath($path) -eq $paths.config) { $baseline[$path] = $nativeHash }
    }
} else { $nativeHash = $historicalNativeHash }
$authPath = Join-Path $paths.opencodex 'auth.json'
$authHash = (Get-FileHash -LiteralPath $authPath).Hash
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-lifecycle-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($evidence)
$checks = [Collections.Generic.List[string]]::new()
$runs = [Collections.Generic.List[object]]::new()
$report = @{ status='running'; startedAt=[DateTime]::UtcNow.ToString('o'); evidence=$evidence; checks=$checks; processRuns=$runs; rebootPerformed=$false }
$report.nativeBaseline = @{ snapshot=$NativeBaselinePath; historicalHash=$historicalNativeHash; cycleHash=$nativeHash; historicalMatch=($historicalNativeHash -eq $nativeHash) }
$needsReconnect = $false
function Assert-Live([bool]$Condition,[string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message); Write-Host "PASS: $Message"
}
function Assert-Preservation([switch]$Native) {
    foreach ($path in $baseline.Keys) {
        if (-not $Native -and [IO.Path]::GetFullPath($path) -eq [IO.Path]::GetFullPath($paths.config)) { continue }
        Assert-Live ((Get-FileHash -LiteralPath $path).Hash -eq $baseline[$path]) ('Baseline preserved: ' + $path)
    }
    Assert-Live ((Get-FileHash -LiteralPath $authPath).Hash -eq $authHash) 'Independent xAI OAuth remains unchanged'
}
function Assert-Connected {
    $check = & $installer -SubscriptionsOnly -Mode Check
    Assert-Live ($check.status -eq 'ready') 'Installed component reports ready'
    foreach ($pair in @(@($paths.configLink,$paths.configSource),@($paths.roleLink,$paths.roleSource))) {
        $link = Get-Item -LiteralPath $pair[0]
        Assert-Live ($link.LinkType -eq 'SymbolicLink' -and $link.Target -eq $pair[1]) ('Direct source link: ' + $pair[0])
    }
    $ready = Invoke-RestMethod ('http://127.0.0.1:' + $state.port + '/readyz') -NoProxy -DisableKeepAlive -TimeoutSec 3
    Assert-Live ($ready.status -eq 'ready') 'Actual managed proxy readiness succeeds'
    return [int]$ready.pid
}
function Invoke-NativeRead([string[]]$Arguments,[string]$Label) {
    $launcher = Join-Path $codexDirectory 'harness/bin/codex.ps1'
    Assert-Live ((Get-Command codex).Source -eq $launcher) 'Ordinary codex still resolves to the harness launcher'
    $prefix = Join-Path $evidence $Label
    $request = @{ executable=$state.dependency.powershell; arguments=@('-NoLogo','-NoProfile','-File',$launcher)+$Arguments
        workingDirectory=$evidence; stdoutPath=$prefix+'.stdout'; stderrPath=$prefix+'.stderr'
        environment=@{CODEX_HOME=$codexDirectory}; memoryLimitMiB=768; timeoutSeconds=30 }
    $request | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath ($prefix+'.request.json') -Encoding utf8
    $result = & (Join-Path $repo 'tools/opencodex-process.ps1') -RequestPath ($prefix+'.request.json') -ResultPath ($prefix+'.result.json') -PassThru
    $runs.Add(@{name=$Label; result=$result})
    Assert-Live ($result.ExitCode -eq 0 -and $result.AssignedBeforeResume) ('Bounded external native command succeeds: ' + $Label)
    return [IO.File]::ReadAllText($request.stdoutPath)
}
try {
    $initialPid = Assert-Connected
    Assert-Preservation
    if ($RunReadinessProbe) {
        $report.scenario = 'global-readiness-only'
        $report.initialPid = $initialPid
        $report.samples = [Collections.Generic.List[object]]::new()
        $observation = [Diagnostics.Stopwatch]::StartNew()
        $version = Invoke-NativeRead @('--version') 'connected-version'
        Assert-Live ($version -match 'codex-cli') 'Native Codex starts with subscriptions connected'
        $mcp = Invoke-NativeRead @('mcp','list','--json') 'connected-mcp' | ConvertFrom-Json
        foreach ($name in @('serena','codebase-memory','graphify','nuphus','harness-lsp')) {
            Assert-Live (@($mcp.name | Where-Object { $_ -match $name }).Count -ge 1) ('Existing MCP remains discoverable: ' + $name)
        }
        $features = Invoke-NativeRead @('features','list') 'connected-features'
        Assert-Live ($features -match '(?m)^multi_agent_v2\s+\S+(?:\s+\S+)*\s+false\s*$') 'Connected global native CLI retains heterogeneous v1 mode'
        do {
            $sample = @{ elapsedSeconds = [Math]::Round($observation.Elapsed.TotalSeconds,2); time = [DateTime]::UtcNow.ToString('o') }
            $check = & $installer -SubscriptionsOnly -Mode Check
            $sample.status = $check.status
            $receipt = & $module { param($p) Get-SubscriptionStarted $p } $paths
            $sample.sameProcess = $receipt -and $receipt.processId -eq $initialPid
            if ($sample.sameProcess) {
                $process = Get-Process -Id $initialPid -ErrorAction SilentlyContinue
                if ($process) { $sample.privateBytes = $process.PrivateMemorySize64; $sample.workingSetBytes = $process.WorkingSet64 }
            }
            $report.samples.Add($sample)
            $report | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $evidence 'report.json') -Encoding utf8
            Assert-Live ($sample.status -eq 'ready' -and $sample.sameProcess) 'Same owned service remains ready during observation'
            if ($observation.Elapsed.TotalSeconds -ge $ObserveSeconds) { break }
            Start-Sleep -Seconds 10
        } while ($true)
        Assert-Preservation
        $report.status = 'passed'
        return
    }
    $repeat = & $installer -SubscriptionsOnly -Mode Install
    Assert-Live ($repeat.status -eq 'ready' -and $repeat.task -eq $state.task) 'Repeat Install retains one task identity and succeeds'
    $priorPid = Assert-Connected
    $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect()
    $folder = $scheduler.GetFolder('\')
    $registered = $folder.GetTask($state.task)
    $current = Get-Content -LiteralPath $paths.state -Raw | ConvertFrom-Json -AsHashtable
    Assert-Live ($registered.Xml -ceq $current.task_xml) 'Exact owned task XML matches before stopping'
    $priorProcess = Get-Process -Id $priorPid
    $needsReconnect = $true
    $registered.Stop(0)
    Assert-Live ($priorProcess.WaitForExit(10000)) 'Stopping the owned supervisor terminates its Bun job'
    $recovery = & $installer -SubscriptionsOnly -Mode Recover
    Assert-Live ($recovery.status -eq 'native-recovered-subscriptions-stopped') 'Recover restores native routing after task stop'
    Assert-Live (-not (Test-Path -LiteralPath $paths.roleLink)) 'Stopped routing exposes no active Grok role'
    Assert-Preservation -Native
    $version = Invoke-NativeRead @('--version') 'native-stopped-version'
    Assert-Live ($version -match 'codex-cli') 'Native Codex starts while the proxy is stopped'
    [void]$registered.Run($null)
    & $module { param($p,$port) Wait-SubscriptionReady $p $port } $paths $state.port
    $restartPid = Assert-Connected
    Assert-Live ($restartPid -ne $priorPid) 'Managed task restart creates a new ready proxy and restores the role'
    $needsReconnect = $false
    $disconnected = & $installer -SubscriptionsOnly -Mode Disconnect
    $needsReconnect = $true
    Assert-Live ($disconnected.status -eq 'disconnected') 'Disconnect succeeds through the delivered installer'
    Assert-Live (-not (Test-Path -LiteralPath $paths.roleLink) -and -not (Test-Path -LiteralPath $paths.configLink)) 'Disconnect removes only the owned subscription links'
    Assert-Preservation -Native
    $mcp = Invoke-NativeRead @('mcp','list','--json') 'native-disconnected-mcp' | ConvertFrom-Json
    $names = @($mcp.name)
    foreach ($name in @('serena','codebase-memory','graphify','nuphus','harness-lsp')) {
        Assert-Live (@($names | Where-Object { $_ -match $name }).Count -ge 1) ('Existing MCP remains discoverable: ' + $name)
    }
    $reconnected = & $installer -SubscriptionsOnly -Mode Install
    Assert-Live ($reconnected.status -eq 'ready') 'Reconnect succeeds with retained authorization'
    $finalPid = Assert-Connected
    $needsReconnect = $false
    $features = Invoke-NativeRead @('features','list') 'reconnected-features'
    Assert-Live ($features -match '(?m)^multi_agent_v2\s+\S+(?:\s+\S+)*\s+false\s*$') 'Reconnected global native CLI retains heterogeneous v1 mode'
    Assert-Preservation
    $report.status='passed'; $report.initialPid=$initialPid; $report.restartPid=$restartPid; $report.finalPid=$finalPid
} catch {
    $report.status='failed'; $report.failure=$_.Exception.Message
    throw
} finally {
    if ($needsReconnect) {
        try {
            & $installer -SubscriptionsOnly -Mode Recover | Out-Null
            & $installer -SubscriptionsOnly -Mode Install | Out-Null
            $cleanupCheck = & $installer -SubscriptionsOnly -Mode Check
            if ($cleanupCheck.status -ne 'ready') { throw 'Reconnect did not retain a ready subscription installation.' }
            $report.cleanup='reconnected through installer'
        } catch { $report.cleanup='Recovery remains pending; inspect installer journals and private logs.'; Write-Warning $report.cleanup }
    }
    $report.completedAt=[DateTime]::UtcNow.ToString('o')
    $report | ConvertTo-Json -Depth 15 | Set-Content -LiteralPath (Join-Path $evidence 'report.json') -Encoding utf8
    Write-Host "Private lifecycle evidence: $evidence"
}
