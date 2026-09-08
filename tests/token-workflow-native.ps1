#requires -Version 7.4
<#
Global native acceptance in an owned external fixture. -TrustHook uses normal
native TUI review for the single owned RTK definition. -RunModelProbes makes two
Astra subscription turns, explicitly named before prompts to avoid title models.
Writes only fixture/evidence, native session state and RTK raw logs. Does not
change providers, credentials, install tools or bypass hook trust.
#>
[CmdletBinding()]
param([switch]$TrustHook,[switch]$RunModelProbes,[string]$EvidenceRoot)
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$liveHome=if($env:CODEX_HOME){$env:CODEX_HOME}else{Join-Path $env:USERPROFILE '.codex'}
$registration=Get-Content (Join-Path $liveHome 'harness/installation.json') -Raw | ConvertFrom-Json
$native=(Get-ChildItem (Join-Path (Split-Path $registration.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor') -Filter codex.exe -Recurse -File | Select-Object -First 1).FullName
if(-not $EvidenceRoot){$EvidenceRoot=Join-Path $env:TEMP ('harness-token-native-'+[guid]::NewGuid().ToString('N'))}
$EvidenceRoot=[IO.Path]::GetFullPath($EvidenceRoot)
if((Test-Path $EvidenceRoot) -or -not $EvidenceRoot.StartsWith([IO.Path]::GetFullPath($env:TEMP),[StringComparison]::OrdinalIgnoreCase)){throw 'EvidenceRoot must be a new directory under TEMP.'}
$workspace=Join-Path $EvidenceRoot 'workspace'
[void][IO.Directory]::CreateDirectory($workspace)
$report=@{passed=$false;native=$native;cwd=$workspace;checks=@{};runs=@()}
$server=$null;$terminal=$null
$oldPath=$env:PATH;$oldHome=$env:CODEX_HOME
function Save-NativeReport { $report | ConvertTo-Json -Depth 25 | Set-Content (Join-Path $EvidenceRoot 'report.json') -Encoding utf8 }
try {
    $env:CODEX_HOME=$liveHome
    $env:PATH=((Join-Path $liveHome 'harness/bin')+';'+(($oldPath -split ';' | Where-Object {$_ -notmatch '(?i)\\WindowsApps(?:\\|$)'}) -join ';'))
    $server=Start-ConsumerServer $native $liveHome $workspace
    $config=Invoke-ConsumerRpc $server 'config/read' @{cwd=$workspace}
    $report.checks.configuration=@{model=$config.config.model;effort=$config.config.model_reasoning_effort;features=$config.config.features}
    $hooks=Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    $registered=@($hooks.data | ForEach-Object {$_.hooks})
    if($registered.Count -ne 1 -or ($registered | ConvertTo-Json -Depth 20 -Compress) -notmatch 'harness-rtk.exe hook'){throw 'Expected exactly the owned RTK hook; inspect other definitions before trust.'}
    Stop-ConsumerServer $server;$server=$null
    if(@($registered | Where-Object trustStatus -ne 'trusted').Count -and $TrustHook){
        Add-Type -Path (Join-Path $PSScriptRoot 'ConPty.cs')
        # Trust is machine-local base state. Selecting the linked profile here
        # would make the native editor write a machine-specific hash to source.
        $terminal=[Harness.Tests.ConPty]::new($native,('"'+$native+'" --no-alt-screen'),$workspace)
        $until=[DateTime]::UtcNow.AddSeconds(45);$reviewed=$false
        while([DateTime]::UtcNow -lt $until){
            $plain=$terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]','' -replace '\s',''
            if($plain -match '2\.Trustallandcontinue'){$terminal.Send("$([char]27)[B");Start-Sleep -Milliseconds 200;$terminal.Send("`r");$reviewed=$true;break}
            if($plain -match 'Doyoutrust'){$terminal.Send("`r");Start-Sleep -Milliseconds 600}
            if($terminal.HasExited){throw 'Native TUI exited before hook review.'}
            Start-Sleep -Milliseconds 200
        }
        if(-not $reviewed){throw 'Native hook review was not observed.'}
        Start-Sleep -Milliseconds 1500
        $terminal.Send('/quit')
        Start-Sleep -Milliseconds 600
        $terminal.Send("`r")
        if(-not $terminal.Wait(20000)){throw 'Native trust TUI did not exit naturally.'}
        [IO.File]::WriteAllText((Join-Path $EvidenceRoot 'trust-terminal.txt'),$terminal.Transcript)
        $terminal.Dispose();$terminal=$null
    }
    $server=Start-ConsumerServer $native $liveHome $workspace
    $hooks=Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    $registered=@($hooks.data | ForEach-Object {$_.hooks})
    $report.checks.hooks=$registered
    if($registered.Count -ne 1 -or @($registered | Where-Object trustStatus -ne 'trusted').Count){throw 'RTK definition has not been natively trusted.'}
    $features=& $native -C $workspace features list
    if(-not ($features -match '^hooks\s+.+true\s*$') -or -not ($features -match '^code_mode\s+.+true\s*$')){throw 'Native selected features are not active.'}
    $report.checks.features=@($features | Where-Object {$_ -match '^(hooks|code_mode)\s'})
    & git init -q $workspace
    for($i=0;$i -lt 80;$i++){
        & git -C $workspace -c user.name=Fixture -c user.email=fixture@example.invalid commit --allow-empty -qm ('receipt-{0:d4}' -f $i)
        if($LASTEXITCODE){throw 'Owned Git fixture preparation failed.'}
    }
    $raw=& git -C $workspace log -n 80 | Out-String
    [IO.File]::WriteAllText((Join-Path $EvidenceRoot 'git-log-raw.txt'),$raw)
    $report.checks.rawBytes=[Text.Encoding]::UTF8.GetByteCount($raw)
    if($RunModelProbes){
        $prompt='Use the installed token-efficient-workflow skill. In this owned fixture, use Code Mode to run exactly two independent shell calls in one batch: harness-rtk.exe exec git log -n 80 and Get-Content missing-owned.txt. Report the latest commit subject and whether the second call failed. Preserve errors in the batch result. Do not create the missing file, write source, delegate, browse, inspect session state or change settings. Return JSON with latest and missing only.'
        $schema=@{type='object';properties=@{latest=@{type='string'};missing=@{type='boolean'}};required=@('latest','missing');additionalProperties=$false}
        foreach($effort in @('low','xhigh')){
            $log=Join-Path $EvidenceRoot ($effort+'-events.jsonl')
            $server.TracePath=$log
            $thread=Invoke-ConsumerRpc $server 'thread/start' @{cwd=$workspace;model='gpt-6-astra';approvalPolicy='never';sandbox='danger-full-access';config=@{model_reasoning_effort=$effort}}
            $id=$thread.thread.id
            $null=Invoke-ConsumerRpc $server 'thread/name/set' @{threadId=$id;name=('Token workflow acceptance '+$effort)}
            $timer=[Diagnostics.Stopwatch]::StartNew()
            $turn=Invoke-ConsumerRpc $server 'turn/start' @{threadId=$id;effort=$effort;input=@(@{type='text';text=$prompt});outputSchema=$schema}
            $until=[DateTime]::UtcNow.AddSeconds(240);$completed=$null
            while([DateTime]::UtcNow -lt $until){
                $line=$server.Process.StandardOutput.ReadLineAsync()
                $remaining=[Math]::Max(1,[int]($until-[DateTime]::UtcNow).TotalMilliseconds)
                if(-not $line.Wait($remaining) -or $null -eq $line.Result){throw 'Native turn stream ended or timed out.'}
                [IO.File]::AppendAllText($log,$line.Result+"`n")
                $event=$line.Result | ConvertFrom-Json -AsHashtable
                if($event.method -eq 'turn/completed' -and $event.params.threadId -eq $id){$completed=$event.params.turn;break}
            }
            $timer.Stop()
            if(-not $completed -or $completed.status -ne 'completed'){throw 'Native turn did not complete successfully.'}
            $saved=Invoke-ConsumerRpc $server 'thread/read' @{threadId=$id;includeTurns=$true}
            $final=@($saved.thread.turns[-1].items | Where-Object type -eq 'agentMessage')[-1].text
            $answer=$final | ConvertFrom-Json -AsHashtable
            if($answer.latest -ne 'receipt-0079' -or $answer.missing -ne $true){throw 'Independent native answer oracle failed.'}
            $rollout=Get-Content -LiteralPath $saved.thread.path -Raw
            if($rollout -notmatch 'rtk raw' -or $rollout -notmatch 'missing-owned.txt'){throw 'Native transcript lacks compressed output or failed-call evidence.'}
            $rows=@($rollout -split '\r?\n' | Where-Object {$_} | ForEach-Object {$_ | ConvertFrom-Json -AsHashtable})
            $contexts=@($rows | Where-Object type -eq 'turn_context')
            if(-not @($contexts | Where-Object { $_.payload.effort -eq $effort -and $_.payload.model -eq 'gpt-6-astra' }).Count){throw 'Effective native Astra effort was not observed.'}
            $codeCalls=@($rows | Where-Object { $_.type -eq 'response_item' -and $_.payload.type -in @('custom_tool_call','function_call') -and $_.payload.name -match '(^|[._])exec$' })
            if(-not @($codeCalls | Where-Object { ([string]$_.payload.input+[string]$_.payload.arguments) -match 'Promise\.all(?:Settled)?' }).Count){throw 'No executed Code Mode independent batch was observed.'}
            $report.runs+=@{effort=$effort;thread=$id;rollout=$saved.thread.path;elapsedSeconds=$timer.Elapsed.TotalSeconds;answer=$answer;events=$log}
            Save-NativeReport
            Write-Output "Native $effort oracle passed."
        }
    }
    $neighbor=Join-Path $EvidenceRoot 'other-project'
    [void][IO.Directory]::CreateDirectory($neighbor)
    $otherHooks=Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($neighbor)}
    $otherRegistered=@($otherHooks.data | ForEach-Object {$_.hooks})
    if($otherRegistered.Count -ne 1 -or @($otherRegistered | Where-Object trustStatus -ne 'trusted').Count){throw 'Trusted RTK selection was not global in a second outside root.'}
    $report.checks.otherRoot=$neighbor
    $report.passed=$true
} finally {
    if($terminal){[IO.File]::WriteAllText((Join-Path $EvidenceRoot 'trust-terminal.txt'),$terminal.Transcript);$terminal.Dispose()}
    Stop-ConsumerServer $server
    $env:PATH=$oldPath;$env:CODEX_HOME=$oldHome
    Save-NativeReport
    [pscustomobject]@{passed=$report.passed;evidence=(Join-Path $EvidenceRoot 'report.json')}
}
