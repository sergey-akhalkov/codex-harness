#requires -Version 7.4
<#
Opt-in native acceptance: one Astra parent, one globally configured Grok middle.
Uses the existing bounded Windows Job runner; never starts/restarts the proxy.
Only a fresh temporary fixture is writable. Private evidence is retained.
RevalidateEvidence checks the same artifacts/transcripts without model calls.
#>
[CmdletBinding()]
param([switch]$RunModelProbes, [string]$RevalidateEvidence, [string]$Python,
    [string]$ProxyBaseUrl,
    [ValidateRange(60,600)][int]$TimeoutSeconds = 300)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $RunModelProbes -and -not $RevalidateEvidence) { 'SKIP: pass -RunModelProbes for live Grok continuation acceptance.'; return }
if (-not $Python -or -not (Test-Path -LiteralPath $Python -PathType Leaf)) { throw 'Supply an installed Python 3 executable with -Python.' }
if ($ProxyBaseUrl -and $ProxyBaseUrl -notmatch '^http://127\.0\.0\.1:[0-9]+/v1$') { throw 'An isolated proxy override must be an explicit loopback v1 URL.' }
$repo = Split-Path $PSScriptRoot -Parent
$codexDirectory = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$launcher = Join-Path $codexDirectory 'harness/bin/codex.ps1'
if ((Get-Command codex).Source -ine $launcher) { throw 'Ordinary codex must resolve to the global harness launcher.' }
$evidence = if ($RevalidateEvidence) { [IO.Path]::GetFullPath($RevalidateEvidence) } else {
    Join-Path ([IO.Path]::GetTempPath()) ('codex-grok-continuation-' + [guid]::NewGuid().ToString('N'))
}
$workspace = Join-Path $evidence 'fixture'
if ($workspace.StartsWith([IO.Path]::GetFullPath($repo).TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Acceptance workspace must be outside the repository.' }
$report = @{status='running';evidence=$evidence;workspace=$workspace;launcher=$launcher;checks=@();startedAt=[DateTime]::UtcNow.ToString('o')}
$report.proxyOverride = $ProxyBaseUrl
function Assert-Probe([bool]$Value,[string]$Message) {
    if (-not $Value) { throw "FAIL: $Message" }
    $report.checks += $Message
    Write-Output "PASS: $Message"
}
function Read-Rows([string]$Path) {
    if ((Get-Item -LiteralPath $Path).Length -gt 16MB) { throw 'Transcript exceeds 16 MiB parsing limit.' }
    Get-Content -LiteralPath $Path | ForEach-Object { if ($_.Trim()) { $_ | ConvertFrom-Json -AsHashtable -Depth 100 } }
}
function Find-Rollout([string]$Id) {
    if ($Id -notmatch '^[a-f0-9-]{36}$') { throw 'Missing native thread identity.' }
    $days = @([datetime]$report.startedAt,[DateTime]::Now,[DateTime]::UtcNow) | ForEach-Object { $_.ToString('yyyy/MM/dd',[Globalization.CultureInfo]::InvariantCulture); $_.ToLocalTime().ToString('yyyy/MM/dd',[Globalization.CultureInfo]::InvariantCulture) } | Sort-Object -Unique
    $paths = @(foreach ($day in $days) {
        $dir = Join-Path $codexDirectory ('sessions/' + $day)
        if (Test-Path -LiteralPath $dir) { Get-ChildItem -LiteralPath $dir -Filter "*-$Id.jsonl" -File | Select-Object -ExpandProperty FullName }
    })
    if ($paths.Count -ne 1) { throw 'Expected one new native rollout.' }
    $paths[0]
}
try {
    if ($RevalidateEvidence) {
        $prior = Get-Content -LiteralPath (Join-Path $evidence 'report.json') -Raw | ConvertFrom-Json -AsHashtable
        $report.startedAt = $prior.startedAt
        $report.proxyOverride = $prior.proxyOverride
        $report.sourceHashes = $prior.sourceHashes
        $report.fixtureHashes = $prior.fixtureHashes
    } else {
        [void][IO.Directory]::CreateDirectory($workspace)
        $inputData = @{marker=('GROK_RESULT_' + [guid]::NewGuid().ToString('N'));numbers=@((Get-Random -Minimum 100 -Maximum 999),(Get-Random -Minimum 100 -Maximum 999),(Get-Random -Minimum 100 -Maximum 999))}
        [IO.File]::WriteAllText((Join-Path $workspace 'input.json'),($inputData | ConvertTo-Json))
        [IO.File]::WriteAllText((Join-Path $workspace 'delayed-read.ps1'),"Start-Sleep -Seconds 3`nGet-Content -LiteralPath (Join-Path `$PSScriptRoot 'input.json') -Raw`n")
        [IO.File]::WriteAllText((Join-Path $workspace 'delayed-failure.ps1'),"Start-Sleep -Seconds 3`nWrite-Output 'GROK_EXPECTED_FAILURE'`nexit 7`n")
        $brief = @'
Bounded fixture task in the current directory, not repository development. Own only result.json. First use functions.exec with pragma // @exec: {"yield_time_ms":1000} and text(await tools.exec_command({cmd:"./delayed-read.ps1",max_output_tokens:1000}));. Retrieve the yielded result with functions.wait before dependent work. Then similarly run ./delayed-failure.ps1 using an exec pragma yield_time_ms 1000; retrieve its actual output and exit code. The second script deliberately exits nonzero; do not alter or rerun either script. From the observed first result, calculate the sum of numbers and write result.json containing marker, sum, failure_exit_code from observed results. Read back and check your output. Return the exact marker, sum and observed deliberate failure exit code in a nonempty final. Do not read input.json directly; use the delayed script. All other files are read-only; no network, MCP, skills setup, agents or other work is needed for this tiny fixture. Follow the loaded tool continuation contract and do not stop with a promise to wait.
'@
        $prompt = 'Native bounded integration check. Spawn exactly one agent_type middle, fork_context=false, with the following complete brief. Do not override its model or modify its brief. Do not inspect fixture files or implement its task yourself. Wait with bounded native waits until it completes, then relay its actual final and close the child. Do not send_input, interrupt, nudge, retry or use backup. No other work. Brief: ' + $brief
        $cliArguments = @('-NoLogo','-NoProfile','-File',$launcher,'exec','--skip-git-repo-check','--json','--model','gpt-6-astra')
        if ($ProxyBaseUrl) { $cliArguments += @('-c',('openai_base_url="' + $ProxyBaseUrl + '"')) }
        $request = @{executable=(Join-Path $PSHOME 'pwsh.exe');arguments=($cliArguments + $prompt)
            workingDirectory=$workspace;stdoutPath=(Join-Path $evidence 'stdout.jsonl');stderrPath=(Join-Path $evidence 'stderr.txt')
            startedPath=(Join-Path $evidence 'started.json');memoryLimitMiB=2048;timeoutSeconds=$TimeoutSeconds}
        [IO.File]::WriteAllText((Join-Path $evidence 'request.json'),($request | ConvertTo-Json -Depth 10))
        $report.sourceHashes = @{}
        foreach ($relative in @('global/harness.config.toml','global/opencodex/config.json','global/opencodex/agents/middle.toml')) {
            $report.sourceHashes[$relative] = (Get-FileHash -LiteralPath (Join-Path $repo $relative)).Hash
        }
        $report.fixtureHashes = @{}
        foreach ($name in @('input.json','delayed-read.ps1','delayed-failure.ps1')) { $report.fixtureHashes[$name] = (Get-FileHash -LiteralPath (Join-Path $workspace $name)).Hash }
        $null = & (Join-Path $repo 'tools/opencodex-process.ps1') -RequestPath (Join-Path $evidence 'request.json') -ResultPath (Join-Path $evidence 'process.json') -PassThru
    }
    $report.process = Get-Content -LiteralPath (Join-Path $evidence 'process.json') -Raw | ConvertFrom-Json -AsHashtable
    Assert-Probe ($report.process.AssignedBeforeResume -and $report.process.Status -eq 'exited' -and $report.process.ExitCode -eq 0) 'contained native process exits successfully'
    $events = @(Read-Rows (Join-Path $evidence 'stdout.jsonl'))
    $thread = @($events | Where-Object { $_.type -eq 'thread.started' })
    Assert-Probe ($thread.Count -eq 1) 'one native parent thread'
    $report.parentRollout = Find-Rollout $thread[0].thread_id
    $parent = @(Read-Rows $report.parentRollout)
    $items = @($parent | Where-Object { $_.type -eq 'event_msg' -and $_.payload.type -eq 'item_completed' } | ForEach-Object { $_.payload.item })
    $spawns = @($items | Where-Object { $_.type -eq 'CollabAgentToolCall' -and $_.tool -eq 'spawn_agent' })
    Assert-Probe ($spawns.Count -eq 1 -and $spawns[0].model -eq 'xai/grok-4.6' -and $spawns[0].receiver_agents[0].agent_role -eq 'middle') 'exactly one globally selected Grok middle'
    $spawnCode = @($parent | Where-Object { $_.type -eq 'response_item' -and $_.payload.type -eq 'custom_tool_call' -and $_.payload.input -match 'tools\.\w*spawn_agent\s*\(' })
    Assert-Probe ($spawnCode.Count -eq 1 -and $spawnCode[0].payload.input -match 'fork_context["'']?\s*:\s*false\b') 'native spawn explicitly requests a fresh context'
    $childId = $spawns[0].receiver_agents[0].thread_id
    $report.childRollout = Find-Rollout $childId
    $child = @(Read-Rows $report.childRollout)
    $meta = @($child | Where-Object { $_.type -eq 'session_meta' })
    Assert-Probe ($meta.Count -eq 1 -and $meta[0].payload.source.subagent.thread_spawn.parent_thread_id -eq $thread[0].thread_id) 'child belongs to the observed parent'
    $models = @($parent | Where-Object { $_.type -eq 'turn_context' } | ForEach-Object { $_.payload.model } | Sort-Object -Unique)
    Assert-Probe ($models.Count -eq 1 -and $models[0] -eq 'gpt-6-astra') 'parent uses only Astra'
    Assert-Probe (@($items | Where-Object { $_.type -eq 'CommandExecution' }).Count -eq 0) 'parent performs no fixture work'
    Assert-Probe (@($items | Where-Object { $_.type -eq 'CollabAgentToolCall' -and $_.tool -in @('send_input','resume_agent') }).Count -eq 0) 'no parent nudges or restarts'
    $inputData = Get-Content -LiteralPath (Join-Path $workspace 'input.json') -Raw | ConvertFrom-Json -AsHashtable
    $result = Get-Content -LiteralPath (Join-Path $workspace 'result.json') -Raw | ConvertFrom-Json -AsHashtable
    $sum = ($inputData.numbers | Measure-Object -Sum).Sum
    $commands = @($child | Where-Object { $_.type -eq 'event_msg' -and $_.payload.type -eq 'item_completed' -and $_.payload.item.type -eq 'CommandExecution' } | ForEach-Object { $_.payload.item })
    $failures = @($commands | Where-Object { $_.exit_code -ne $null -and $_.exit_code -ne 0 -and $_.aggregated_output -match 'GROK_EXPECTED_FAILURE' -and ($_.command -join ' ') -match 'delayed-failure\.ps1' })
    Assert-Probe ($failures.Count -eq 1) 'delayed failure executes once and its actual process result remains visible'
    $report.observedFailureExit = $failures[0].exit_code
    # PowerShell -Command may map a script's exit 7 to process exit 1. The
    # native process receipt, not an assumed wrapper exit, owns this assertion.
    Assert-Probe ($result.marker -ceq $inputData.marker -and $result.sum -eq $sum -and $result.failure_exit_code -eq $report.observedFailureExit) 'dependent artifact contains actual random marker, correct sum and observed failure exit'
    $final = @($parent | Where-Object { $_.type -eq 'event_msg' -and $_.payload.type -eq 'task_complete' })
    Assert-Probe ($final.Count -eq 1 -and $final[0].payload.last_agent_message.Contains($inputData.marker)) 'parent receives the actual child result'
    $childText = [IO.File]::ReadAllText($report.childRollout)
    Assert-Probe ($childText.Contains('Call functions.wait with only') -and $childText.Contains('Script running with cell ID')) 'new role contract loaded and delayed execution exercised'
    $oracle = & $Python -B (Join-Path $PSScriptRoot 'grok-continuation-oracle.py') --rollout $report.childRollout --marker $inputData.marker --require-clean
    Assert-Probe ($LASTEXITCODE -eq 0) 'continuation oracle accepts native child with no tool argument errors'
    $report.oracle = $oracle | ConvertFrom-Json -AsHashtable
    $fixtureHashes = if ($RevalidateEvidence) { $prior.fixtureHashes } else { $report.fixtureHashes }
    foreach ($name in $fixtureHashes.Keys) { Assert-Probe ((Get-FileHash -LiteralPath (Join-Path $workspace $name)).Hash -ceq $fixtureHashes[$name]) "fixture preserved: $name" }
    $report.status = 'passed'
} catch {
    $report.status = 'failed'
    $report.failure = $_.Exception.Message
    throw
} finally {
    $report.completedAt = [DateTime]::UtcNow.ToString('o')
    $name = if ($RevalidateEvidence) { 'revalidated.json' } else { 'report.json' }
    [IO.File]::WriteAllText((Join-Path $evidence $name),($report | ConvertTo-Json -Depth 20))
    Write-Output "Private evidence: $evidence"
}
