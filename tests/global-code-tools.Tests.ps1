#requires -Version 7.4
# Exercise the existing global registrations from a new native consumer in an
# empty, owned directory outside the checkout. No configuration is written.
# Nuphus evaluates arithmetic in its own lazily launched empty browser context.
[CmdletBinding()]
param([string]$CodexHome = (Join-Path $env:USERPROFILE '.codex'), [string]$EvidenceRoot, [switch]$ReadPublicRepository)
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot '../tools/code-tools.psm1')
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$runtime = Get-HarnessCodeToolsRuntime $env:USERPROFILE $CodexHome ''
if (-not $EvidenceRoot) { $EvidenceRoot = Join-Path $CodexHome ('harness/verification/global-mcp-' + [guid]::NewGuid().ToString('N')) }
if (Test-Path -LiteralPath $EvidenceRoot) { throw 'Use a new owned evidence directory.' }
$workspace = Join-Path $EvidenceRoot 'neutral'
[IO.Directory]::CreateDirectory($workspace) | Out-Null
$configBefore = Get-Content -LiteralPath (Join-Path $CodexHome 'config.toml') -Raw
$server = $null
$checks = [Collections.Generic.List[string]]::new()
function Assert-Global([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message)
    Write-Host "PASS: $Message"
}
try {
    $server = Start-ConsumerServer $runtime.native $CodexHome $workspace
    $listed = Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    $hooks = @($listed.data | ForEach-Object { $_.hooks })
    Assert-Global ($hooks.Count -eq 7 -and @($hooks | Where-Object trustStatus -NE 'trusted').Count -eq 0) 'new native consumer loads all seven trusted global hooks'
    $thread = Invoke-ConsumerRpc $server 'thread/start' @{cwd=$workspace;ephemeral=$true}
    foreach ($request in @(
        @{server='serena';tool='initial_instructions';arguments=@{};expected='Serena'},
        @{server='codebase-memory';tool='list_projects';arguments=@{};expected='projects'},
        @{server='graphify';tool='graph_stats';arguments=@{};expected='Nodes:'},
        @{server='nuphus';tool='browser_evaluate';arguments=@{script='6 * 7';confirm=$true};expected='42'}
    )) {
        $result = Invoke-ConsumerRpc $server 'mcpServer/tool/call' @{threadId=$thread.thread.id;server=$request.server;tool=$request.tool;arguments=$request.arguments} -TimeoutSeconds 100
        $encoded = $result | ConvertTo-Json -Depth 30
        [IO.File]::WriteAllText((Join-Path $EvidenceRoot ($request.server + '.json')), $encoded)
        Assert-Global (-not $result.isError -and $encoded.Contains($request.expected)) ("global " + $request.server + ' performs its real operation from the neutral directory')
    }
    if ($ReadPublicRepository) {
        $repo = Join-Path $EvidenceRoot 'explicit repo кириллица'
        [IO.Directory]::CreateDirectory($repo) | Out-Null
        & git init -q --initial-branch=main $repo
        if ($LASTEXITCODE) { throw 'Owned worktree initialization failed.' }
        & git -C $repo remote add origin https://github.com/Graphify-Labs/graphify.git
        if ($LASTEXITCODE) { throw 'Owned worktree remote declaration failed.' }
        $prs = Invoke-ConsumerRpc $server 'mcpServer/tool/call' @{threadId=$thread.thread.id;server='graphify';tool='list_prs';arguments=@{repo=$repo;base='main'}} -TimeoutSeconds 100
        $prs | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'graphify-explicit-repo.json')
        Assert-Global (-not $prs.isError -and ($prs.content.text -join '') -match 'Open PRs targeting main|No open PRs') 'global Graphify queries the explicit Unicode worktree independently of the neutral consumer cwd'
    }
    Assert-Global (@(Get-ChildItem -LiteralPath $workspace -Force).Count -eq 0) 'the neutral workspace needs no project-local registration or generated source files'
} finally {
    if ($server) { Stop-ConsumerServer $server }
}
$compareInput = @{before=$configBefore;after=(Get-Content -LiteralPath (Join-Path $CodexHome 'config.toml') -Raw);workspace=$workspace} | ConvertTo-Json -Compress
$compareScript = @'
import json,sys,tomllib
v=json.load(sys.stdin); before=tomllib.loads(v['before']); after=tomllib.loads(v['after'])
own=v['workspace'].replace('/','\\').lower()
for k in list(after.get('projects',{})):
 if k.replace('/','\\').lower()==own and k not in before.get('projects',{}):
  assert after['projects'][k]=={'trust_level':'trusted'}
  del after['projects'][k]
assert before==after, 'Native consumer changed settings beyond its own workspace trust'
print('preserved')
'@
$comparison = $compareInput | & $runtime.python -B -c $compareScript
Assert-Global ($LASTEXITCODE -eq 0 -and $comparison -eq 'preserved') 'native consumer preserves existing settings and only adds its own directory trust'
$report = @{nativeVersion=(& $runtime.native --version);checks=$checks;workspace=$workspace;codexHome=$CodexHome}
$report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $EvidenceRoot 'report.json')
$report | ConvertTo-Json -Depth 10
