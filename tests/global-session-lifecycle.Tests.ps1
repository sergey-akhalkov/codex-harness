#requires -Version 7.4
# Two bounded model-backed read turns using the real linked global CLI, followed
# by an app-server history fork and MCP read. No project-local registration.
param([switch]$RunAgent, [int]$ExpectedGraphNodes = 88513,
    [ValidateSet('low','medium','high','xhigh')][string]$ReasoningEffort)
$ErrorActionPreference = 'Stop'
if (-not $RunAgent) { throw 'Pass -RunAgent for the explicit model-backed consumer checks.' }
Import-Module (Join-Path $PSScriptRoot '../tools/code-tools.psm1')
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$codexDirectory = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
$runtime = Get-HarnessCodeToolsRuntime $env:USERPROFILE $codexDirectory ''
$evidence = Join-Path $codexDirectory ('harness/verification/session-lifecycle-' + [guid]::NewGuid().ToString('N'))
$workspace = Join-Path $evidence 'neutral'
[IO.Directory]::CreateDirectory($workspace) | Out-Null
$checks = [Collections.Generic.List[string]]::new()
function Assert-Session([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message; inspect $evidence" }
    $checks.Add($Message); Write-Host "PASS: $Message"
}
function Invoke-GlobalExec([string[]]$Arguments, [string]$Name) {
    $start = [Diagnostics.ProcessStartInfo]::new($runtime.powershell)
    foreach ($argument in @('-NoLogo','-NoProfile','-File',(Join-Path $codexDirectory 'harness/bin/codex.ps1')) + $Arguments) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $workspace
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.Environment['CODEX_HOME'] = $codexDirectory
    $process = [Diagnostics.Process]::Start($start)
    $stdout = $process.StandardOutput.ReadToEndAsync(); $stderr = $process.StandardError.ReadToEndAsync()
    $process.StandardInput.Close()
    try {
        if (-not $process.WaitForExit(180000)) { $process.Kill($true); throw "Global $Name turn exceeded its bounded timeout." }
        $output = $stdout.GetAwaiter().GetResult()
        [IO.File]::WriteAllText((Join-Path $evidence ($Name + '.jsonl')), $output)
        [IO.File]::WriteAllText((Join-Path $evidence ($Name + '-private-stderr.txt')), $stderr.GetAwaiter().GetResult())
        Assert-Session ($process.ExitCode -eq 0) ("global CLI $Name exits successfully")
        @($output -split '\r?\n' | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
    } finally { $process.Dispose() }
}
$prompt = 'Call the actual MCP tool mcp__graphify__graph_stats exactly once and report its result. Discover through ALL_TOOLS in functions.exec if metadata is deferred. Do not run shell or edit files or manually call diagnostic tools. This is a read-only global integration acceptance probe. Report the observed Nodes count as GLOBAL_GRAPH_NODES=<number> in your final answer; preserve actual failure if the call fails.'
function Assert-GraphCall($Events, [string]$Name) {
    $calls = @($Events | Where-Object { $_.type -eq 'item.completed' -and $_.item.type -eq 'mcp_tool_call' -and $_.item.server -eq 'graphify' -and $_.item.tool -eq 'graph_stats' })
    $final = @($Events | Where-Object { $_.type -eq 'item.completed' -and $_.item.type -eq 'agent_message' } | ForEach-Object { $_.item.text }) -join "`n"
    Assert-Session ($calls.Count -eq 1 -and $calls[0].item.status -eq 'completed' -and $null -eq $calls[0].item.error -and $final -match ('GLOBAL_GRAPH_NODES=' + $ExpectedGraphNodes + '\b')) ("$Name receives the actual saved graph through the global MCP registration")
}
$modelOptions = if ($ReasoningEffort) { @('-c',('model_reasoning_effort="' + $ReasoningEffort + '"')) } else { @() }
$initial = Invoke-GlobalExec (@('exec','--skip-git-repo-check','--json') + $modelOptions + @($prompt)) 'exec'
$threadId = @($initial | Where-Object type -EQ 'thread.started')[0].thread_id
Assert-Session ([bool]$threadId) 'ordinary exec creates a persisted native session'
Assert-GraphCall $initial 'exec'
$resumed = Invoke-GlobalExec (@('exec','resume',$threadId,'--json','--skip-git-repo-check') + $modelOptions + @($prompt)) 'resume'
Assert-GraphCall $resumed 'resume'
$server = Start-ConsumerServer $runtime.native $codexDirectory $workspace
try {
    $fork = Invoke-ConsumerRpc $server 'thread/fork' @{threadId=$threadId;cwd=$workspace} -TimeoutSeconds 60
    Assert-Session ($fork.thread.id -ne $threadId) 'unprofiled native consumer forks the persisted CLI history'
    $stats = Invoke-ConsumerRpc $server 'mcpServer/tool/call' @{threadId=$fork.thread.id;server='graphify';tool='graph_stats';arguments=@{}} -TimeoutSeconds 90
    Assert-Session (-not $stats.isError -and ($stats.content.text -join '').Contains('Nodes: ' + $ExpectedGraphNodes)) 'forked session inherits the global graph connection'
} finally { Stop-ConsumerServer $server }
Assert-Session (@(Get-ChildItem -LiteralPath $workspace -Force).Count -eq 0) 'all three entry points preserve the neutral directory without local registrations'
@{nativeVersion=(& $runtime.native --version);checks=$checks;threadId=$threadId;evidence=$evidence} | ConvertTo-Json -Depth 10 | Tee-Object -FilePath (Join-Path $evidence 'report.json')
