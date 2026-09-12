#requires -Version 7.4
<#
Opt-in Windows Scheduler/lifecycle test, without credentials or model calls.
Uses the installed pinned package read-only, real component functions and real
2048 MiB service jobs. Private fixture wrappers isolate every home before import
and reject external fetch/HTTP/socket calls; source config uses a free port,
static models and disabled sidecars. This is not authenticated/global acceptance.
A contained worker has a 330-second ceiling (510 with recovery probes). The outer supervisor removes only
the exact task whose action points at its unique fixture. Evidence is retained;
no recursive deletion or global configuration/service mutations are performed.
#>
[CmdletBinding()]
param([switch]$RunIsolatedProbes, [switch]$RunRecoveryProbes, [string]$PackageRoot, [string]$WorkerConfig)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $RunIsolatedProbes) { 'SKIP: pass -RunIsolatedProbes to run the isolated, credential-free scheduler probe.'; return }
if (-not $IsWindows) { throw 'The isolated scheduled lifecycle probe requires Windows.' }
$repository = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$module = Import-Module (Join-Path $repository 'tools/subscription-routing.psm1') -Force -PassThru
function Write-ProbeJson([string]$Path,$Value) { [IO.File]::WriteAllText($Path,($Value | ConvertTo-Json -Depth 30)) }
function Read-ProbeJson([string]$Path) { Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable }

if (-not $WorkerConfig) {
    $realUser = [Environment]::GetFolderPath('UserProfile')
    $realCodex = if ($env:CODEX_HOME) { [IO.Path]::GetFullPath($env:CODEX_HOME) } else { Join-Path $realUser '.codex' }
    $realPaths = & $module { param($r,$u,$c) Get-SubscriptionPaths $r $u $c } $repository $realUser $realCodex
    $dependency = & $module { param($p) Get-SubscriptionDependency $p '' } $realPaths
    if ($PackageRoot) {
        $PackageRoot = [IO.Path]::GetFullPath($PackageRoot)
        $metadata = Read-ProbeJson (Join-Path $PackageRoot 'package.json')
        if ($metadata.name -ne '@bitkyc08/opencodex' -or $metadata.version -ne '2.44.0') { throw 'The isolated probe requires the already installed pinned package.' }
        $dependency = @{root=$PackageRoot;version='2.44.0';bun=(Join-Path $PackageRoot 'node_modules/bun/bin/bun.exe');cli=(Join-Path $PackageRoot 'src/cli/index.ts');powershell=(& $module { Resolve-SubscriptionPowerShell })}
    }
    if (-not $dependency) { throw 'Install the pinned dependency first; this probe never downloads packages.' }
    $fixture = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-isolated-' + [guid]::NewGuid().ToString('N'))
    [void][IO.Directory]::CreateDirectory($fixture)
    $parameters = @{fixture=$fixture;repository=$repository;dependency=$dependency;realUser=$realUser;source=(Join-Path $fixture 'source');user=(Join-Path $fixture 'user');codex=(Join-Path $fixture 'codex')}
    $paths = & $module { param($p) Get-SubscriptionPaths $p.source $p.user $p.codex } $parameters
    if ($paths.task -eq $realPaths.task) { throw 'Isolated task identity collided with the global installation.' }
    $parameters.task=$paths.task
    $parameters.recovery=[bool]$RunRecoveryProbes
    $workerPath=Join-Path $fixture 'worker-config.json'; Write-ProbeJson $workerPath $parameters
    $protected = @($realPaths.config, (Join-Path $realCodex 'auth.json'), (Join-Path $realCodex 'harness.config.toml'),
        (Join-Path $realUser '.opencodex/config.json'), (Join-Path $realUser '.opencodex/auth.json'),
        (Join-Path $realUser '.config/opencode/opencode.json'), (Join-Path $realUser '.config/opencode/opencode.jsonc'),
        (Join-Path $realUser '.local/share/opencode/auth.json'))
    $baseline=@{}
    foreach($path in $protected) { $baseline[$path]=if(Test-Path -LiteralPath $path -PathType Leaf){(Get-FileHash -LiteralPath $path).Hash}else{$null} }
    Write-ProbeJson (Join-Path $fixture 'global-hashes-before.json') $baseline
    $request=@{executable=$dependency.powershell;arguments=@('-NoLogo','-NoProfile','-File',$PSCommandPath,'-RunIsolatedProbes','-WorkerConfig',$workerPath)
        workingDirectory=$fixture;stdoutPath=(Join-Path $fixture 'worker.stdout');stderrPath=(Join-Path $fixture 'worker.stderr');memoryLimitMiB=2048;timeoutSeconds=$(if($RunRecoveryProbes){510}else{330});environment=@{}}
    Write-ProbeJson (Join-Path $fixture 'worker.request.json') $request
    $summary=@{status='running';fixture=$fixture;task=$paths.task;startedAt=[DateTime]::UtcNow.ToString('o');globalPreservation=@{};cleanup='pending'}
    Write-Output "Isolated lifecycle evidence: $fixture"
    try {
        $summary.worker = & (Join-Path $repository 'tools/opencodex-process.ps1') -RequestPath (Join-Path $fixture 'worker.request.json') -ResultPath (Join-Path $fixture 'worker.result.json') -PassThru
        if ($summary.worker.ExitCode -ne 0) { throw "Isolated worker failed: $($summary.worker.Status), exit $($summary.worker.ExitCode); inspect its retained report." }
        $summary.status='passed'
    } catch { $summary.status='failed';$summary.failure=$_.Exception.Message }
    finally {
        try {
            $task = & $module {param($name) Get-SubscriptionTask $name} $paths.task
            if ($task) {
                # Retain the pre-cleanup native host boundary when no host log
                # was created; a readiness timeout does not identify its cause.
                $summary.taskBeforeCleanup=$task
                $summary.fixtureProcesses=@(Get-CimInstance Win32_Process | Where-Object {
                    $_.CommandLine -and $_.CommandLine.Contains($fixture)
                } | Select-Object ProcessId,ParentProcessId,CreationDate,ExecutablePath,CommandLine)
                [xml]$xml=$task.xml
                $actions=@($xml.Task.Actions.Exec)
                $expectedArgs='-NoLogo -NoProfile -WindowStyle Hidden -File "' + (Join-Path $paths.source 'tools/opencodex-service.ps1') + '" -StatePath "' + $paths.service + '"'
                if($actions.Count -ne 1 -or $actions[0].Command -cne $dependency.powershell -or $actions[0].Arguments -cne $expectedArgs -or $actions[0].WorkingDirectory -cne $paths.source){throw 'Foreign task action preserved during isolated cleanup.'}
                & $module {param($p,$xml) Set-SubscriptionTask $p.task $null $xml $p} $paths $task.xml
            }
            $summary.cleanup='exact fixture task absent; service job terminated'
        } catch {$summary.cleanup=$_.Exception.Message;$summary.status='failed'}
        foreach($path in $baseline.Keys){
            $hash=if(Test-Path -LiteralPath $path -PathType Leaf){(Get-FileHash -LiteralPath $path).Hash}else{$null}
            $summary.globalPreservation[$path]=($hash -ceq $baseline[$path])
            if($hash -cne $baseline[$path]){$summary.status='failed'}
        }
        $summary.completedAt=[DateTime]::UtcNow.ToString('o')
        Write-ProbeJson (Join-Path $fixture 'report.json') $summary
        Write-Output "Isolated lifecycle result: $($summary.status); report: $(Join-Path $fixture 'report.json')"
    }
    if($summary.status -ne 'passed'){throw 'The isolated lifecycle did not pass; retained evidence includes cleanup and global preservation.'}
    return
}

if(-not [IO.Path]::IsPathFullyQualified($WorkerConfig)){throw 'Worker config must be absolute.'}
$parameters=Read-ProbeJson $WorkerConfig
$fixture=[IO.Path]::GetFullPath($parameters.fixture)
if((Split-Path $fixture) -ine [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') -or (Split-Path $fixture -Leaf) -notmatch '^codex-subscription-isolated-[a-f0-9]{32}$' -or $WorkerConfig -cne (Join-Path $fixture 'worker-config.json')){throw 'Invalid isolated worker boundary.'}
foreach($name in @('source','user','codex')){if([IO.Path]::GetFullPath($parameters[$name]) -cne (Join-Path $fixture $name)){throw 'Worker paths escape the fixture.'};[void][IO.Directory]::CreateDirectory($parameters[$name])}
$paths = & $module {param($p) Get-SubscriptionPaths $p.source $p.user $p.codex} $parameters
$checks=[Collections.Generic.List[string]]::new();$timeline=[Collections.Generic.List[object]]::new()
$report=@{status='running';startedAt=[DateTime]::UtcNow.ToString('o');checks=$checks;timeline=$timeline;fixture=$fixture;credentialFree=$true;rebootPerformed=$false;subscriptionModuleHash=(Get-FileHash -LiteralPath (Join-Path $repository 'tools/subscription-routing.psm1')).Hash;limitations=@('Private source config uses free loopback port and static model catalog.','CLI/helper import wrappers isolate homes and reject external networking.','Component entry point and scheduled host are real; authenticated/global acceptance is not established.')}
function Assert-Probe([bool]$Value,[string]$Message){if(-not $Value){throw "FAIL: $Message"};$checks.Add($Message);Write-Output "PASS: $Message"}
function Invoke-ProbeMode([string]$Mode){Invoke-HarnessSubscriptionRouting -Mode $Mode -SourceRoot $paths.source -UserHome $paths.user -CodexHome $paths.codex}
function Save-ProbeReport { Write-ProbeJson (Join-Path $fixture 'lifecycle.json') $report }
function Assert-Native {
    Assert-Probe ((Get-FileHash -LiteralPath $paths.config).Hash -ceq $nativeHash) 'Native Codex config restored byte for byte'
    Assert-Probe ((Get-Item -LiteralPath $nativeAgentsLink).LinkType -eq 'SymbolicLink') 'Permanent native agent link survives subscription restoration'
    foreach($level in $nativeAgentHashes.Keys) {
        Assert-Probe ((Get-FileHash -LiteralPath (Join-Path $nativeAgentsLink "$level.toml")).Hash -ceq $nativeAgentHashes[$level]) "Permanent Astra level remains intact: $level"
    }
}
function Get-HttpComparison([int]$ProcessId) {
    $comparison=@{time=[DateTime]::UtcNow.ToString('o');pid=$ProcessId;clients=@{}}
    foreach($kind in @('ordinary','direct')) {
        $elapsed=[Diagnostics.Stopwatch]::StartNew()
        try {
            $response=if($kind -eq 'ordinary'){Invoke-RestMethod "http://127.0.0.1:$port/readyz" -TimeoutSec 3}else{Invoke-RestMethod "http://127.0.0.1:$port/readyz" -TimeoutSec 3 -NoProxy -DisableKeepAlive}
            $comparison.clients[$kind]=@{ready=($response.status -eq 'ready' -and $response.pid -eq $ProcessId);milliseconds=$elapsed.ElapsedMilliseconds}
        }catch{$comparison.clients[$kind]=@{ready=$false;failure=$_.Exception.GetType().Name;milliseconds=$elapsed.ElapsedMilliseconds}}
    }
    $elapsed=[Diagnostics.Stopwatch]::StartNew()
    $curlOutput=& (Join-Path $env:SystemRoot 'System32/curl.exe') --silent --noproxy '*' --max-time 3 "http://127.0.0.1:$port/readyz"
    $curlExit=$LASTEXITCODE
    $curlReady=$false
    if($curlExit -eq 0){try{$response=($curlOutput -join '')|ConvertFrom-Json;$curlReady=$response.status -eq 'ready' -and $response.pid -eq $ProcessId}catch{$comparison.curlParseFailure=$_.Exception.GetType().Name}}
    $comparison.clients.curl=@{ready=$curlReady;exitCode=$curlExit;milliseconds=$elapsed.ElapsedMilliseconds}
    $live=Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    $comparison.processAlive=[bool]$live
    if($live){$comparison.privateBytes=$live.PrivateMemorySize64;$comparison.processorSeconds=$live.TotalProcessorTime.TotalSeconds}
    $tcp=[Net.Sockets.TcpClient]::new()
    try{$comparison.tcpAccepts=$tcp.ConnectAsync('127.0.0.1',$port).Wait(1000) -and $tcp.Connected}catch{$comparison.tcpAccepts=$false}finally{$tcp.Dispose()}
    [IO.File]::AppendAllText((Join-Path $fixture 'http-comparison.jsonl'),($comparison|ConvertTo-Json -Depth 8 -Compress)+[Environment]::NewLine)
    $comparison
}
function Assert-Connected {
    $check=Invoke-ProbeMode Check
    if ($check.status -ne 'ready') {
        # Preserve the failed check before cleanup changes the observed state.
        $started = & $module { param($p) Get-SubscriptionStarted $p } $paths
        $detail = @{check=$check;time=[DateTime]::UtcNow.ToString('o');started=$started
            task=(& $module {param($p) Get-SubscriptionTask $p.task} $paths)
            runtime=(Read-ProbeJson (Join-Path $paths.opencodex 'runtime-port.json'))
            links=@{}}
        foreach ($name in @('configLink','roleLink')) {
            $detail.links[$name]=& $module {param($p) Get-SubscriptionLink $p} $paths[$name]
        }
        if ($started) { $detail.http=Get-HttpComparison ([int]$started.processId) }
        Write-ProbeJson (Join-Path $fixture 'failed-ready-check.json') $detail
    }
    Assert-Probe ($check.status -eq 'ready') 'Actual component reports ready'
    foreach($name in @('configLink','roleLink')){$target=if($name -eq 'configLink'){$paths.configSource}else{$paths.roleSource};Assert-Probe ((& $module {param($p) Get-SubscriptionLink $p} $paths[$name]) -ceq $target) "Direct source link: $name"}
    $ready=Invoke-RestMethod "http://127.0.0.1:$port/readyz" -TimeoutSec 3 -NoProxy -DisableKeepAlive
    return [int]$ready.pid
}
try {
    foreach($directory in @((Join-Path $paths.source 'tools'),$paths.roleSource,$paths.opencodex,(Join-Path $paths.user '.config'),(Join-Path $paths.user '.local/share'))){[void][IO.Directory]::CreateDirectory($directory)}
    $listener=[Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,0)
    try{$listener.Start();$port=$listener.LocalEndpoint.Port}finally{$listener.Stop()}
    Assert-Probe ($port -ne 10100 -and $port -ge 1024) 'Unique non-global loopback port selected'
    $report.port=$port
    $config=Read-ProbeJson (Join-Path $repository 'global/opencodex/config.json')
    $config.port=$port;$config.providers.xai.liveModels=$false
    $config.tokenGuardian=@{enabled=$false};$config.webSearchSidecar=@{enabled=$false};$config.visionSidecar=@{enabled=$false}
    Write-ProbeJson $paths.configSource $config
    Copy-Item -LiteralPath (Join-Path $repository 'global/opencodex/dependency.json') -Destination (Join-Path $paths.source 'global/opencodex/dependency.json')
    foreach($role in Get-ChildItem -LiteralPath (Join-Path $repository 'global/opencodex/agents') -Filter '*.toml'){Copy-Item -LiteralPath $role.FullName -Destination $paths.roleSource}
    $nativeAgentsLink=Join-Path $paths.codex 'agents/codex-harness'
    [void][IO.Directory]::CreateDirectory((Split-Path $nativeAgentsLink))
    $nativeAgentSource=Join-Path $paths.user 'owned-agent-source'
    [void][IO.Directory]::CreateDirectory($nativeAgentSource)
    [IO.File]::WriteAllText((Join-Path $nativeAgentSource 'native_sentinel.toml'), "name = `"native_sentinel`"`r`ndescription = `"Owned preservation fixture`"`r`nmodel = `"gpt-6-astra`"`r`n")
    $null=New-Item -ItemType SymbolicLink -Path $nativeAgentsLink -Target $nativeAgentSource
    $nativeAgentHashes=@{}
    foreach($level in @('native_sentinel')){$nativeAgentHashes[$level]=(Get-FileHash -LiteralPath (Join-Path $nativeAgentsLink "$level.toml")).Hash}
    [IO.File]::WriteAllText($paths.config,"# isolated native sentinel`r`nmodel = `"gpt-6-astra`"`r`nmodel_reasoning_effort = `"high`"`r`n")
    $nativeHash=(Get-FileHash -LiteralPath $paths.config).Hash
    $isolation=@{USERPROFILE=$paths.user;HOME=$paths.user;HOMEDRIVE=([IO.Path]::GetPathRoot($paths.user).TrimEnd('\'));HOMEPATH=$paths.user.Substring(2);APPDATA=(Join-Path $paths.user 'AppData/Roaming');LOCALAPPDATA=(Join-Path $paths.user 'AppData/Local');XDG_CONFIG_HOME=(Join-Path $paths.user '.config');XDG_DATA_HOME=(Join-Path $paths.user '.local/share');CODEX_HOME=$paths.codex;CODEX_SQLITE_HOME=$paths.codex;OPENCODEX_HOME=$paths.opencodex;CLAUDE_CONFIG_DIR=(Join-Path $paths.user '.claude')}
    $isolation.OCX_REAL_HOME=$parameters.realUser;$isolation.OCX_TEST_HOME_GUARD='1'
    Write-ProbeJson (Join-Path $fixture 'isolation.json') $isolation
    # No credentials are read/copied. Clear ambient credential and path overrides
    # in the new Bun process before the first upstream module import.
    $guardSource=@'
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {syncBuiltinESMExports} from 'node:module';
const fixture = path.dirname(fileURLToPath(import.meta.url));
const isolated = JSON.parse(fs.readFileSync(path.join(fixture, 'isolation.json'),'utf8'));
for (const key of Object.keys(process.env)) {
  if (/(?:TOKEN|SECRET|PASSWORD|API_KEY|BASE_URL|PROXY)/i.test(key) || /^(?:CODEX|OPENCODE|GROK|CLAUDE|XAI)_/.test(key)) delete process.env[key];
}
Object.assign(process.env, isolated);
if (path.resolve(os.homedir()).toLowerCase() !== path.resolve(isolated.USERPROFILE).toLowerCase()) throw new Error('Native homedir escaped fixture');
for (const location of [path.join(isolated.CODEX_HOME,'auth.json'),path.join(isolated.OPENCODEX_HOME,'auth.json'),path.join(isolated.XDG_DATA_HOME,'opencode/auth.json')]) {
  if(fs.existsSync(location)) throw new Error('Credential-free fixture unexpectedly contains authorization');
}
const audit=path.join(fixture,'network.jsonl');
const reject = kind => { fs.appendFileSync(audit,JSON.stringify({time:new Date().toISOString(),kind,blocked:true})+'\n'); throw new Error('Isolated fixture blocked external network'); };
const local = value => ['127.0.0.1','localhost','::1','[::1]'].includes(value);
const originalFetch=globalThis.fetch;
globalThis.fetch=(input,init)=>{const url=new URL(typeof input==='string'||input instanceof URL?input:input.url);if(!local(url.hostname))return Promise.reject().catch(()=>reject('fetch'));return originalFetch(input,{...init,redirect:'error'});};
for(const name of ['node:http','node:https']) {const mod=(await import(name)).default;mod.request=()=>reject(name);mod.get=()=>reject(name);}
const net=(await import('node:net')).default;
const originalConnect=net.Socket.prototype.connect;
net.Socket.prototype.connect=function(...args){const options=Array.isArray(args[0])?args[0][0]:args[0];const host=typeof options==='object'?options.host:(typeof args[1]==='string'?args[1]:'localhost');if(host&&!local(host))return reject('node:net');return originalConnect.apply(this,args);};
const originalBunConnect=Bun.connect;
Bun.connect=function(options){if(options.hostname&&!local(options.hostname))return reject('Bun.connect');return originalBunConnect.call(Bun,options);};
syncBuiltinESMExports();
fs.appendFileSync(path.join(fixture,'guard.jsonl'),JSON.stringify({time:new Date().toISOString(),pid:process.pid,homedirIsolated:true})+'\n');
'@
    [IO.File]::WriteAllText((Join-Path $fixture 'guard.mjs'),$guardSource)
    $guardUri=([uri](Join-Path $fixture 'guard.mjs')).AbsoluteUri
    foreach($name in @('opencodex-config-check.mjs','opencodex-native-restore.mjs')){
        $originalUri=([uri](Join-Path $repository ('tools/'+$name))).AbsoluteUri
        [IO.File]::WriteAllText((Join-Path $paths.source ('tools/'+$name)),('await import(' + ($guardUri|ConvertTo-Json -Compress) + '); await import(' + ($originalUri|ConvertTo-Json -Compress) + ');'))
    }
    Copy-Item -LiteralPath (Join-Path $repository 'tools/opencodex-process.ps1') -Destination (Join-Path $paths.source 'tools/opencodex-process.ps1')
    Copy-Item -LiteralPath (Join-Path $repository 'tools/opencodex-process.cs') -Destination (Join-Path $paths.source 'tools/opencodex-process.cs')
    $originalService=(Join-Path $repository 'tools/opencodex-service.ps1').Replace("'","''")
    $serviceWrapper=@'
#requires -Version 7.4
param([string]$StatePath)
$wrapperLog=Join-Path (Split-Path $StatePath) 'runs/wrapper.jsonl'
function Write-WrapperReceipt([string]$Stage,$Failure=$null) {
    $entry=@{stage=$Stage;pid=$PID;time=[DateTime]::UtcNow.ToString('o')}
    if($Failure){$entry.exceptionType=$Failure.Exception.GetType().FullName;$entry.stack=$Failure.ScriptStackTrace}
    [IO.File]::AppendAllText($wrapperLog,(($entry | ConvertTo-Json -Compress)+[Environment]::NewLine))
}
Write-WrapperReceipt 'entry'
try { & '__ORIGINAL_SERVICE__' -StatePath $StatePath }
catch { Write-WrapperReceipt 'failed' $_; throw }
'@
    $serviceWrapper=$serviceWrapper.Replace('__ORIGINAL_SERVICE__',$originalService)
    [IO.File]::WriteAllText((Join-Path $paths.source 'tools/opencodex-service.ps1'),$serviceWrapper)
    $cliWrapper=Join-Path $fixture 'cli.mjs'
    $cliUri=([uri]$parameters.dependency.cli).AbsoluteUri
    [IO.File]::WriteAllText($cliWrapper,('await import(' + ($guardUri|ConvertTo-Json -Compress) + '); await import(' + ($cliUri|ConvertTo-Json -Compress) + ');'))
    $dependency=$parameters.dependency.Clone();$dependency.cli=$cliWrapper
    & $module {
        param($dependency)
        $script:isolatedDependency=$dependency
        function script:Get-SubscriptionDependency {
            param($Paths,$CodexCommand)
            $fixtureRoot = Split-Path $script:isolatedDependency.cli -Parent
            if ($CodexCommand -or $Paths.source -ne (Join-Path $fixtureRoot 'source') -or
                $Paths.user -ne (Join-Path $fixtureRoot 'user') -or $Paths.codex -ne (Join-Path $fixtureRoot 'codex')) {
                throw 'Dependency fixture received paths outside its isolated homes or an unexpected Codex command.'
            }
            $script:isolatedDependency.Clone()
        }
        function script:Initialize-SubscriptionDependency {throw 'Isolated fixture never installs packages.'}
    } $dependency
    $started=Invoke-ProbeMode Install
    Assert-Probe ($started.status -eq 'ready') 'Install starts actual isolated scheduled service'
    $initialPid=@(Assert-Connected)[-1];$report.initialPid=$initialPid
    $idle=[Diagnostics.Stopwatch]::StartNew()
    do {
        $sample=@{elapsedSeconds=[Math]::Round($idle.Elapsed.TotalSeconds,2);time=[DateTime]::UtcNow.ToString('o');ready=$false}
        try{
            $process=Get-Process -Id $initialPid
            $sample.privateBytes=$process.PrivateMemorySize64;$sample.workingSetBytes=$process.WorkingSet64
            $sample.processorSeconds=$process.TotalProcessorTime.TotalSeconds
            $sample.httpComparison=Get-HttpComparison $initialPid
            # Retain the ordinary-client observation when it times out while
            # both independent direct clients work; this does not attribute the
            # difference to a specific proxy or connection reuse mechanism.
            $sample.ready=$sample.httpComparison.clients.direct.ready -and $sample.httpComparison.clients.curl.ready
            try{
                # This is a new token generated by this credential-free local
                # server, not a copied user or upstream subscription credential.
                $headers=@{'x-opencodex-api-key'=[IO.File]::ReadAllText((Join-Path $paths.opencodex 'admin-api-token')).Trim()}
                $memory=Invoke-RestMethod "http://127.0.0.1:$port/api/system/memory" -Headers $headers -TimeoutSec 3 -NoProxy -DisableKeepAlive
                $sample.memory=@{}
                foreach($key in @('rss','heapUsed','heapTotal','external','arrayBuffers','observedBytes','activeTurnCount','uptimeSeconds')){if($memory.PSObject.Properties[$key]){$sample.memory[$key]=$memory.$key}}
                if($memory.PSObject.Properties['responseState']){$sample.memory.responseStateTotalBytes=$memory.responseState.totalBytes}
                if($memory.PSObject.Properties['appOwnedBytes']){$sample.memory.appOwnedRetainedBytes=$memory.appOwnedBytes.retainedBytes}
            }catch{$sample.memoryUnavailable=$_.Exception.GetType().Name}
        }catch{$sample.failure=$_.Exception.GetType().Name}
        $timeline.Add($sample);Save-ProbeReport
        if(-not $sample.ready){throw 'Isolated proxy lost readiness during the 120-second idle stability sample.'}
        if($idle.Elapsed.TotalSeconds -ge 120){break}
        Start-Sleep -Seconds 10
    }while($true)
    Assert-Probe ($idle.Elapsed.TotalSeconds -ge 120) 'Actual isolated proxy remains ready beyond 120 seconds'
    $repeat=Invoke-ProbeMode Install
    Assert-Probe ($repeat.status -eq 'ready' -and $repeat.task -eq $started.task) 'Repeat Install retains one task identity'
    $priorPid=@(Assert-Connected)[-1]
    $state=Read-ProbeJson $paths.state
    $scheduler=New-Object -ComObject 'Schedule.Service';$scheduler.Connect();$registered=$scheduler.GetFolder('\').GetTask($paths.task)
    Assert-Probe ($registered.Xml -ceq $state.task_xml) 'Exact owned task XML verified before stopping'
    & $module {param($task,$p) Stop-SubscriptionTaskRuntime $task $p} $registered $paths
    $recover=Invoke-ProbeMode Recover
    Assert-Probe ($recover.status -eq 'native-recovered-subscriptions-stopped') 'Recover restores native routing after actual task stop'
    Assert-Native
    Assert-Probe (-not(Test-Path -LiteralPath $paths.roleLink)) 'Stopped subscription role is absent'
    [void]$registered.Run($null)
    & $module {param($p,$port) Wait-SubscriptionReady $p $port} $paths $port
    $restartPid=@(Assert-Connected)[-1];$report.restartPid=$restartPid
    Assert-Probe ($restartPid -ne $priorPid) 'Task restart creates a new ready process and reconnects its role'
    if($parameters.recovery) {
        # Migrate an actual legacy task in place before crashing this fixture only.
        $registered=$scheduler.GetFolder('\').GetTask($paths.task)
        $legacy=$registered.Definition
        [xml]$legacyXml=$legacy.XmlText
        $restartNode=$legacyXml.Task.Settings.RestartOnFailure
        if($restartNode){[void]$restartNode.ParentNode.RemoveChild($restartNode)}
        $legacy.XmlText=$legacyXml.OuterXml
        $null=$scheduler.GetFolder('\').RegisterTaskDefinition($paths.task,$legacy,36,$null,$null,3,$null)
        $state=Read-ProbeJson $paths.state
        $state.task_xml=$scheduler.GetFolder('\').GetTask($paths.task).Xml
        Write-ProbeJson $paths.state $state
        $configured=Invoke-ProbeMode ConfigureRestart
        Assert-Probe ($configured.status -eq 'subscriptions-restart-policy-configured') 'Native ConfigureRestart updates the existing task'
        Assert-Probe (@(Assert-Connected)[-1] -eq $restartPid) 'Native ConfigureRestart preserves the live proxy PID'
        $registered=$scheduler.GetFolder('\').GetTask($paths.task)
        Assert-Probe ($registered.Definition.Settings.RestartCount -eq 3 -and $registered.Definition.Settings.RestartInterval -eq 'PT1M') 'Native task has three one-minute recovery attempts'
        $crashed=& $module {param($p) Get-SubscriptionOwnedProcess $p} $paths
        if(-not $crashed -or $crashed.Id -ne $restartPid){throw 'Fixture process identity changed before crash injection.'}
        try { $crashed.Kill(); Assert-Probe ($crashed.WaitForExit(10000)) 'Only the attested fixture runtime is terminated' }
        finally { $crashed.Dispose() }
        $recoveryClock=[Diagnostics.Stopwatch]::StartNew()
        $withdrawn=$false;$recoveredPid=0
        do {
            if(-not(Test-Path -LiteralPath $paths.roleLink)){$withdrawn=$true}
            if((& $module {param($p,$port) Test-SubscriptionReady $p $port} $paths $port) -and (Test-Path -LiteralPath $paths.roleLink)) {
                $observed=Invoke-RestMethod "http://127.0.0.1:$port/readyz" -TimeoutSec 3 -NoProxy -DisableKeepAlive
                if($observed.pid -ne $restartPid){$recoveredPid=[int]$observed.pid;break}
            }
            Start-Sleep -Seconds 2
        }while($recoveryClock.Elapsed.TotalSeconds -lt 150)
        Assert-Probe ($withdrawn) 'Failed runtime withdraws its role before automatic recovery'
        Assert-Probe ($recoveredPid -gt 0) 'Scheduled host automatically starts a new attested ready runtime'
        Assert-Probe (@(Assert-Connected)[-1] -eq $recoveredPid) 'Automatic recovery republishes the exact role link'
        $report.automaticRecovery=@{crashedPid=$restartPid;recoveredPid=$recoveredPid;seconds=$recoveryClock.Elapsed.TotalSeconds;roleWithdrawn=$withdrawn}
        Save-ProbeReport
    }
    $disconnect=Invoke-ProbeMode Disconnect
    Assert-Probe ($disconnect.status -eq 'disconnected') 'Disconnect succeeds'
    Assert-Native
    Assert-Probe (-not(Test-Path -LiteralPath $paths.configLink) -and -not(Test-Path -LiteralPath $paths.roleLink)) 'Disconnect removes both owned source links'
    $reconnect=Invoke-ProbeMode Install
    Assert-Probe ($reconnect.status -eq 'ready') 'Reconnect succeeds without credentials'
    $report.finalPid=@(Assert-Connected)[-1]
    Invoke-ProbeMode Disconnect | Out-Null
    Assert-Native
    Assert-Probe (-not(Test-Path -LiteralPath (Join-Path $paths.opencodex 'auth.json'))) 'No OpenCodex authorization was imported or created'
    Assert-Probe (-not(Test-Path -LiteralPath (Join-Path $paths.codex 'auth.json'))) 'No Codex authorization was imported or created'
    $report.status='passed'
} catch { $report.status='failed';$report.failure=$_.Exception.Message;throw }
finally {
    $report.completedAt=[DateTime]::UtcNow.ToString('o')
    $report.processResults=@(Get-ChildItem -LiteralPath $paths.runtime -Filter '*.result.json' -ErrorAction SilentlyContinue | ForEach-Object {Read-ProbeJson $_.FullName})
    $report.serviceLimits=@(Get-ChildItem -LiteralPath $paths.runtime -Filter '*.request.json' -ErrorAction SilentlyContinue | ForEach-Object {
        $request=Read-ProbeJson $_.FullName
        if($request.ContainsKey('startedPath')){@{memoryLimitMiB=$request.memoryLimitMiB;timeoutSeconds=$request.timeoutSeconds;startedPath=$request.startedPath}}
    })
    if($timeline.Count){
        $report.maxObservedPrivateBytes=($timeline | Where-Object {$_.ContainsKey('privateBytes')} | Measure-Object -Property privateBytes -Maximum).Maximum
        $report.maxObservedWorkingSetBytes=($timeline | Where-Object {$_.ContainsKey('workingSetBytes')} | Measure-Object -Property workingSetBytes -Maximum).Maximum
    }
    Save-ProbeReport
}
