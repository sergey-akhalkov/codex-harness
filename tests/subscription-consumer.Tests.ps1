#requires -Version 7.4
<#
Opt-in live acceptance through the installed global harness launcher. This creates
at most two model-backed CLI sessions and one named child, without configuring or
starting OpenCodex. Authenticate and activate the integration before running.

Evidence stays in a unique host temporary directory, including private native
output and references to the new persisted rollouts. No existing sessions are
resumed, archived, deleted or restarted. Only processes started here are stopped.
Each CLI launcher and its descendants start in a 2048 MiB Windows job with the
configured timeout; closing that job also cleans up remaining child processes.
Native model metadata proves selection, not the upstream billing/auth route;
correlate report timestamps/thread ids with separate redacted proxy evidence.
#>
[CmdletBinding()]
param(
    [switch]$RunModelProbes,
    [string]$RevalidateEvidence,
    [string]$GrokModel,
    [ValidateSet('minimal','low','medium','high','xhigh')][string]$GrokReasoningEffort = 'xhigh',
    [string]$ParentModel = 'gpt-6-astra',
    [ValidateSet('All','Main','Delegation')][string]$Scenario = 'All',
    [ValidateRange(30,600)][int]$TimeoutSeconds = 240
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $RunModelProbes -and -not $RevalidateEvidence) {
    Write-Output 'SKIP: pass -RunModelProbes -GrokModel <authenticated model slug> for live subscription checks.'
    return
}
if ([string]::IsNullOrWhiteSpace($GrokModel) -or $GrokModel -match '[\r\n]') {
    throw '-RunModelProbes requires the exact authenticated -GrokModel slug.'
}
if ($ParentModel -eq $GrokModel) { throw 'ParentModel and GrokModel must differ for heterogeneous delegation.' }

$codexDirectory = [IO.Path]::GetFullPath($(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }))
$launcher = Join-Path $codexDirectory 'harness/bin/codex.ps1'
$resolvedCommand = Get-Command codex -ErrorAction Stop
if ($resolvedCommand.CommandType -ne 'ExternalScript' -or
    -not [IO.Path]::GetFullPath($resolvedCommand.Source).Equals([IO.Path]::GetFullPath($launcher), [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Ordinary codex must resolve to the installed global harness launcher before this acceptance test.'
}
$powershell = Join-Path $PSHOME 'pwsh.exe'
if (-not (Test-Path -LiteralPath $powershell -PathType Leaf)) { throw 'This consumer check targets native Windows PowerShell 7.' }
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-consumer-' + [guid]::NewGuid().ToString('N'))
if ($RevalidateEvidence) {
    if (-not [IO.Path]::IsPathFullyQualified($RevalidateEvidence)) { throw 'Revalidation requires an absolute evidence directory.' }
    $evidence = [IO.Path]::GetFullPath($RevalidateEvidence)
}
$workspace = Join-Path $evidence 'fixture'
$repo = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent)).TrimEnd('\','/')
$boundedRunner = Join-Path $repo 'tools/opencodex-process.ps1'
if (-not (Test-Path -LiteralPath $boundedRunner -PathType Leaf)) { throw 'The audited Windows job runner is required for live consumer checks.' }
if ([IO.Path]::GetFullPath($workspace).StartsWith($repo + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'The host temporary directory must be outside this repository.'
}
$null = [IO.Directory]::CreateDirectory($workspace)
$utf8 = [Text.UTF8Encoding]::new($false)
$startedAt = [DateTime]::UtcNow
if ($RevalidateEvidence) {
    $prior = Get-Content -LiteralPath (Join-Path $evidence 'report.json') -Raw | ConvertFrom-Json -AsHashtable
    if ($prior.scenario -ne $Scenario -or $prior.expectedGrokModel -ne $GrokModel -or $prior.codexHome -ne $codexDirectory) { throw 'Retained evidence identity does not match the requested scenario.' }
    $startedAt = [datetime]$prior.startedAt
}
$checks = [Collections.Generic.List[string]]::new()
$scenarios = [Collections.Generic.List[object]]::new()
$processRuns = [Collections.Generic.List[object]]::new()
$report = [ordered]@{
    status = 'running'; startedAt = $startedAt.ToString('o'); evidence = $evidence
    launcher = $launcher; workingDirectory = $workspace; codexHome = $codexDirectory
    scenario = $Scenario; expectedGrokModel = $GrokModel; expectedParentModel = $ParentModel
    modelProcessLimit = $(if ($Scenario -eq 'All') { 2 } else { 1 })
    timeoutSecondsPerProcess = $TimeoutSeconds; checks = $checks; scenarios = $scenarios
    memoryLimitMiBPerProcessTree = 2048; processRuns = $processRuns
    upstreamSubscriptionRoute = 'Requires separate correlated proxy evidence; native provider identity may remain openai.'
    revalidationOnly = [bool]$RevalidateEvidence
}

function Assert-Subscription([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message. Inspect $evidence" }
    $checks.Add($Message)
    Write-Host "PASS: $Message"
}

function Invoke-SubscriptionCli([string[]]$Arguments, [string]$Name, [int]$LimitSeconds = $TimeoutSeconds) {
    $outPath = Join-Path $evidence ($Name + '-private-stdout.jsonl')
    $errPath = Join-Path $evidence ($Name + '-private-stderr.txt')
    $requestPath = Join-Path $evidence ($Name + '-private-process-request.json')
    $resultPath = Join-Path $evidence ($Name + '-process-result.json')
    $startedPath = Join-Path $evidence ($Name + '-process-started.json')
    $request = @{
        executable = $powershell
        arguments = @('-NoLogo','-NoProfile','-File',$launcher) + $Arguments
        workingDirectory = $workspace
        stdoutPath = $outPath; stderrPath = $errPath; startedPath = $startedPath
        environment = @{ CODEX_HOME = $codexDirectory }
        memoryLimitMiB = 2048; timeoutSeconds = $LimitSeconds
    }
    if (-not $RevalidateEvidence) { [IO.File]::WriteAllText($requestPath, ($request | ConvertTo-Json -Depth 8), $utf8) }
    # The runner assigns the suspended launcher before execution, streams output
    # to new files, and closes its kill-on-close job on every completion path.
    $result = if ($RevalidateEvidence) {
        Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    } else {
        & $boundedRunner -RequestPath $requestPath -ResultPath $resultPath -PassThru
    }
    $processRuns.Add(@{ name = $Name; resultPath = $resultPath; startedPath = $startedPath; result = $result })
    Assert-Subscription ($result.AssignedBeforeResume -eq $true -and $result.MemoryLimitBytes -eq 2048MB) "$Name starts inside a 2048 MiB job before execution"
    Assert-Subscription ($result.Status -eq 'exited' -and $result.ExitCode -eq 0) "$Name exits successfully through the global launcher (status $($result.Status), exit $($result.ExitCode))"
    if ((Get-Item -LiteralPath $outPath).Length -gt 16MB -or (Get-Item -LiteralPath $errPath).Length -gt 16MB) {
        throw "Global CLI $Name output exceeds the 16 MiB per-stream parsing limit."
    }
    return [IO.File]::ReadAllText($outPath)
}

function Read-SubscriptionJsonLines([string]$Text) {
    foreach ($line in $Text -split '\r?\n') {
        if (-not [string]::IsNullOrWhiteSpace($line)) { $line | ConvertFrom-Json -AsHashtable -Depth 100 }
    }
}

function Get-SubscriptionRollout([string]$ThreadId) {
    if ($ThreadId -notmatch '^[0-9a-fA-F-]{36}$') { throw 'Native thread id was missing or was not a UUID.' }
    # Inspect filenames only in the dates adjacent to this bounded run. Never
    # search unrelated transcript contents to find the newly created session.
    $days = @(
        foreach ($date in @($startedAt, $startedAt.ToLocalTime(), [DateTime]::UtcNow, [DateTime]::Now)) {
            foreach ($offset in -1..1) { $date.AddDays($offset).ToString('yyyy/MM/dd', [Globalization.CultureInfo]::InvariantCulture) }
        }
    ) | Sort-Object -Unique
    $paths = @(@(
        foreach ($day in $days) {
            $directory = Join-Path $codexDirectory ('sessions/' + $day)
            if (Test-Path -LiteralPath $directory -PathType Container) {
                Get-ChildItem -LiteralPath $directory -File -Filter ("*-$ThreadId.jsonl") | Select-Object -ExpandProperty FullName
            }
        }
    ) | Sort-Object -Unique)
    if (@($paths).Count -ne 1) { throw "Expected one persisted native rollout for $ThreadId; found $(@($paths).Count)." }
    if ((Get-Item -LiteralPath $paths[0]).Length -gt 16MB) { throw "Native rollout $ThreadId exceeds the bounded parsing limit." }
    $rows = @(Read-SubscriptionJsonLines ([IO.File]::ReadAllText($paths[0])))
    $metadata = @($rows | Where-Object { $_['type'] -eq 'session_meta' })
    Assert-Subscription ($metadata.Count -eq 1 -and $metadata[0]['payload']['id'] -eq $ThreadId) "rollout identity matches native thread $ThreadId"
    return @{ path = $paths[0]; rows = $rows; metadata = $metadata[0]['payload'] }
}

function Assert-SubscriptionModel([hashtable]$Rollout, [string]$Expected, [string]$Name) {
    $models = @($Rollout.rows | Where-Object { $_['type'] -eq 'turn_context' } | ForEach-Object { $_['payload']['model'] } | Sort-Object -Unique)
    Assert-Subscription ($models.Count -eq 1 -and $models[0] -ceq $Expected) "$Name native turn_context selects exactly $Expected"
}

function Get-SubscriptionCompletedCommand([hashtable]$Rollout) {
    # Codex 0.153.4 persists CommandExecution inside item_completed. Older
    # rollouts use exec_command_end; prefer the modern set to avoid double counts.
    $modern = @($Rollout.rows | Where-Object {
        $_['type'] -eq 'event_msg' -and $_['payload']['type'] -eq 'item_completed' -and
        $_['payload']['item']['type'] -eq 'CommandExecution'
    } | ForEach-Object { $_['payload']['item'] })
    if ($modern.Count) { return $modern }
    $Rollout.rows | Where-Object {
        $_['type'] -eq 'event_msg' -and $_['payload']['type'] -eq 'exec_command_end'
    } | ForEach-Object { $_['payload'] }
}

function Assert-SubscriptionToolRead([hashtable]$Rollout, [string]$Marker, [string]$Name) {
    $completedCommands = @(Get-SubscriptionCompletedCommand $Rollout)
    $reads = @($completedCommands | Where-Object {
        $_['exit_code'] -eq 0 -and
        (($_ | ConvertTo-Json -Depth 30 -Compress).Contains($Marker))
    })
    Assert-Subscription ($reads.Count -ge 1) "$Name has an actual successful shell result containing the fixture-only marker"
    Assert-Subscription ($completedCommands.Count -le 3) "$Name stays within three completed shell calls"
    $changes = @($Rollout.rows | Where-Object {
        $_['type'] -eq 'event_msg' -and (
            $_['payload']['type'] -in @('patch_apply_begin','patch_apply_end') -or
            ($_['payload']['type'] -eq 'item_completed' -and $_['payload']['item']['type'] -eq 'FileChange')
        )
    })
    Assert-Subscription ($changes.Count -eq 0) "$Name performs no patch operation"
}

function Get-SubscriptionFinal($Events) {
    $messages = @($Events | Where-Object {
        $_['type'] -eq 'item.completed' -and $_['item']['type'] -eq 'agent_message'
    } | ForEach-Object { $_['item']['text'] })
    if (-not $messages.Count) { return '' }
    return [string]$messages[-1]
}

function Get-SubscriptionThread($Events) {
    $starts = @($Events | Where-Object { $_['type'] -eq 'thread.started' })
    Assert-Subscription ($starts.Count -eq 1) 'exec creates exactly one primary native thread'
    return [string]$starts[0]['thread_id']
}

try {
    Write-Host "Private subscription consumer evidence: $evidence"
    $report['nativeVersion'] = (Invoke-SubscriptionCli @('--version') 'version' 30).Trim()
    $features = Invoke-SubscriptionCli @('features','list') 'features' 30
    Assert-Subscription ($features -match '(?m)^multi_agent_v2\s+\S+(?:\s+\S+)*\s+false\s*$') 'installed global configuration disables multi_agent_v2'

    if ($Scenario -in @('All','Main')) {
        $mainMarker = 'MAIN_FIXTURE_' + [guid]::NewGuid().ToString('N')
        $mainFile = Join-Path $workspace 'main-fixture.txt'
        if ($RevalidateEvidence) { $mainMarker = [IO.File]::ReadAllText($mainFile) }
        else { [IO.File]::WriteAllText($mainFile, $mainMarker, $utf8) }
        $mainHash = (Get-FileHash -LiteralPath $mainFile).Hash
        $mainPrompt = 'This is a bounded read-only integration probe, not a development task. Use one local shell tool to read main-fixture.txt in the current directory. Return its exact contents in your final answer. Do not use subagents, network tools, MCP, other files, or write operations. Do not create plans or proposals. Stop immediately after reporting the contents.'
        $options = @('exec','--skip-git-repo-check','--json','--model',$GrokModel)
        if ($GrokReasoningEffort) { $options += @('-c',('model_reasoning_effort="' + $GrokReasoningEffort + '"')) }
        $begin = [DateTime]::UtcNow.ToString('o')
        $mainEvents = @(Read-SubscriptionJsonLines (Invoke-SubscriptionCli ($options + @($mainPrompt)) 'main'))
        $mainId = Get-SubscriptionThread $mainEvents
        $main = Get-SubscriptionRollout $mainId
        Assert-SubscriptionModel $main $GrokModel 'main Grok'
        Assert-SubscriptionToolRead $main $mainMarker 'main Grok'
        Assert-Subscription ((Get-SubscriptionFinal $mainEvents).Contains($mainMarker)) 'main Grok returns the fixture-only marker'
        Assert-Subscription ((Get-FileHash -LiteralPath $mainFile).Hash -eq $mainHash) 'main fixture remains unchanged'
        $scenarios.Add(@{ name = 'main'; startedAt = $begin; completedAt = [DateTime]::UtcNow.ToString('o'); threadId = $mainId; model = $GrokModel; nativeProvider = $main.metadata['model_provider']; rolloutPath = $main.path })
    }

    if ($Scenario -in @('All','Delegation')) {
        $childMarker = 'CHILD_FIXTURE_' + [guid]::NewGuid().ToString('N')
        $childFile = Join-Path $workspace 'review-fixture.txt'
        if ($RevalidateEvidence) { $childMarker = ([IO.File]::ReadAllText($childFile) -split '\r?\n')[0] }
        else { [IO.File]::WriteAllText($childFile, "$childMarker`nRequirement: the result must equal 42.`nObserved result: 41.`n", $utf8) }
        $childHash = (Get-FileHash -LiteralPath $childFile).Hash
        $parentPrompt = 'This is a bounded read-only integration probe, not a development task. Spawn exactly one agent of agent_type middle with a fresh context (no history fork). In its task, instruct it to read review-fixture.txt in the current directory using one local shell call, report the exact marker from that file, and describe the discrepancy between the requirement and the observed result. Tell it not to use network tools, MCP, other files, further agents, or write operations. You must not inspect the file yourself. Do not override the role model. Wait for this agent to finish and relay its actual marker and finding. Do not retry with another role or model if it fails. No other work.'
        $parentPrompt += ' Include this tool contract in the child task: when using functions.exec, print the nested shell result with text(await tools.exec_command(...)); a bare awaited call discards the output. Read the visible output before answering.'
        $begin = [DateTime]::UtcNow.ToString('o')
        $parentEvents = @(Read-SubscriptionJsonLines (Invoke-SubscriptionCli @('exec','--skip-git-repo-check','--json','--model',$ParentModel,$parentPrompt) 'delegation'))
        $parentId = Get-SubscriptionThread $parentEvents
        $parent = Get-SubscriptionRollout $parentId
        Assert-SubscriptionModel $parent $ParentModel 'parent'
        $modernSpawns = @($parent.rows | Where-Object {
            $_['type'] -eq 'event_msg' -and $_['payload']['type'] -eq 'item_completed' -and
            $_['payload']['item']['type'] -eq 'CollabAgentToolCall' -and $_['payload']['item']['tool'] -eq 'spawn_agent'
        } | ForEach-Object { $_['payload']['item'] })
        if ($modernSpawns.Count) {
            Assert-Subscription ($modernSpawns.Count -eq 1 -and $modernSpawns[0]['status'] -eq 'completed') 'parent makes exactly one completed native spawn_agent call'
            $receivers = @($modernSpawns[0]['receiver_agents'])
            Assert-Subscription ($receivers.Count -eq 1 -and $receivers[0]['agent_role'] -ceq 'middle') 'native spawn selects the exact middle role'
            Assert-Subscription ($modernSpawns[0]['model'] -ceq $GrokModel) 'native spawn uses the declared Grok model'
            $childId = [string]$receivers[0]['thread_id']
            # Code-mode calls remain data: inspect the recorded literal flag;
            # never execute JavaScript taken from a model transcript.
            $codeSpawns = @($parent.rows | Where-Object {
                $_['type'] -eq 'response_item' -and $_['payload']['type'] -eq 'custom_tool_call' -and
                $_['payload']['input'] -match 'tools\.\w*spawn_agent\s*\('
            })
            Assert-Subscription ($codeSpawns.Count -eq 1 -and $codeSpawns[0]['payload']['input'] -match '\bfork_context\s*:\s*false\b') 'recorded spawn explicitly requests a fresh context'
        } else {
        $spawns = @($parent.rows | Where-Object {
            $_['type'] -eq 'response_item' -and $_['payload']['type'] -eq 'function_call' -and
            $_['payload']['name'] -match '(^|\.)spawn_agent$'
        })
        Assert-Subscription ($spawns.Count -eq 1) 'parent makes exactly one actual spawn_agent call'
        $spawnArgs = $spawns[0]['payload']['arguments'] | ConvertFrom-Json -AsHashtable
        Assert-Subscription ($spawnArgs['agent_type'] -ceq 'middle') 'actual spawn arguments select the exact middle role'
        Assert-Subscription (-not $spawnArgs['fork_context']) 'Grok task uses a fresh context'
        $spawnOutputs = @($parent.rows | Where-Object {
            $_['type'] -eq 'response_item' -and $_['payload']['type'] -eq 'function_call_output' -and
            $_['payload']['call_id'] -eq $spawns[0]['payload']['call_id']
        })
        Assert-Subscription ($spawnOutputs.Count -eq 1) 'native spawn has one matching result'
        $spawnResult = $spawnOutputs[0]['payload']['output'] | ConvertFrom-Json -AsHashtable
        $childId = [string]$spawnResult['agent_id']
        }
        $child = Get-SubscriptionRollout $childId
        $source = $child.metadata['source']
        Assert-Subscription ($source -is [System.Collections.IDictionary] -and
            $source['subagent']['thread_spawn']['parent_thread_id'] -eq $parentId) 'child native metadata links it to this parent session'
        Assert-SubscriptionModel $child $GrokModel 'named Grok child'
        Assert-SubscriptionToolRead $child $childMarker 'named Grok child'
        $parentCommands = @(Get-SubscriptionCompletedCommand $parent)
        Assert-Subscription ($parentCommands.Count -eq 0) 'parent delegates the fixture read instead of reading it itself'
        $final = Get-SubscriptionFinal $parentEvents
        Assert-Subscription ($final.Contains($childMarker) -and $final -match '\b41\b' -and $final -match '\b42\b') 'parent receives the child marker and the actual review discrepancy'
        Assert-Subscription ((Get-FileHash -LiteralPath $childFile).Hash -eq $childHash) 'review fixture remains unchanged'
        $scenarios.Add(@{ name = 'delegation'; startedAt = $begin; completedAt = [DateTime]::UtcNow.ToString('o'); threadId = $parentId; childThreadId = $childId; role = 'middle'; parentModel = $ParentModel; childModel = $GrokModel; nativeProvider = $child.metadata['model_provider']; parentRolloutPath = $parent.path; childRolloutPath = $child.path })
    }
    $unexpected = @(Get-ChildItem -LiteralPath $workspace -Force | Where-Object { $_.Name -notin @('main-fixture.txt','review-fixture.txt') })
    Assert-Subscription ($unexpected.Count -eq 0) 'external fixture contains no generated files or project-local registrations'
    $report['status'] = 'passed'
} catch {
    $report['status'] = 'failed'
    # Raw provider errors stay in private stderr; avoid copying arbitrary error
    # content into a shareable report or the terminal.
    $report['failure'] = 'Consumer check failed; inspect the recorded checks and private outputs.'
    throw
} finally {
    $report['completedAt'] = [DateTime]::UtcNow.ToString('o')
    $reportName = if ($RevalidateEvidence) { 'revalidated-' + [guid]::NewGuid().ToString('N') + '.json' } else { 'report.json' }
    [IO.File]::WriteAllText((Join-Path $evidence $reportName), ($report | ConvertTo-Json -Depth 20), $utf8)
    Write-Host "Evidence retained: $evidence"
}
