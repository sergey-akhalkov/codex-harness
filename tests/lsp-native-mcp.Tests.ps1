#requires -Version 7.4
# Real global native Codex -> adopted Serena edit -> automatic LSP hook feedback.
# No registration, trust writer, package installation or manual diagnostic call.
[CmdletBinding()]
param([string]$NativeCodex, [string]$CodexHome, [string]$ProbeRoot, [switch]$RunAgent, [switch]$KeepProbe)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$repository = Split-Path -Parent $PSScriptRoot
if (-not $CodexHome) { $CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' } }
$CodexHome = [IO.Path]::GetFullPath($CodexHome)
if (-not $NativeCodex) {
    $installed = Get-Content -LiteralPath (Join-Path $CodexHome 'harness/installation.json') -Raw | ConvertFrom-Json
    $vendor = Join-Path (Split-Path -Parent $installed.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
    $NativeCodex = (Get-ChildItem -LiteralPath $vendor -Recurse -Filter codex.exe | Select-Object -First 1).FullName
}
$inventory = Get-Content -LiteralPath (Join-Path $CodexHome 'harness/code-tools.json') -Raw | ConvertFrom-Json
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { throw 'The adopted Serena interpreter must already exist.' }
if (-not $ProbeRoot) { $ProbeRoot = Join-Path ([IO.Path]::GetTempPath()) ('lsp-native-mcp-' + [guid]::NewGuid().ToString('N')) }
$ProbeRoot = [IO.Path]::GetFullPath($ProbeRoot)
if (Test-Path -LiteralPath $ProbeRoot) { throw 'Use an absent, disposable probe root.' }
if ($ProbeRoot.StartsWith($repository + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'This acceptance fixture must be outside the repository.' }
$workspace = Join-Path $ProbeRoot 'Serena проект with spaces'
$utf8 = [Text.UTF8Encoding]::new($false)
$checks = [Collections.Generic.List[string]]::new()
$server = $null
$succeeded = $false
$failure = $null
$preserved = @{}
$configPath = Join-Path $CodexHome 'config.toml'
$configBefore = Get-Content -LiteralPath $configPath -Raw
$serenaHome = if ($env:SERENA_HOME) { $env:SERENA_HOME } else { Join-Path $env:USERPROFILE '.serena' }
foreach ($path in @((Join-Path $CodexHome 'harness/code-tools-registration.json'),
        (Join-Path $CodexHome 'harness/code-tools.json'), (Join-Path $CodexHome 'hooks.json'), (Join-Path $serenaHome 'serena_config.yml'))) {
    $preserved[$path] = if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path).Hash } else { $null }
}
function Assert-McpLsp([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message)
    Write-Output "PASS: $Message"
}
function Write-McpLsp([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Assert-PreservedMcpLsp {
    foreach ($path in $preserved.Keys) {
        $current = if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path).Hash } else { $null }
        if ($current -ne $preserved[$path]) { throw "Existing configuration changed; preserve it for review: $path" }
    }
    $compareScript = @'
import json,sys,tomllib
sys.stdin.reconfigure(encoding='utf-8-sig')
value=json.load(sys.stdin); before=tomllib.loads(value['before']); after=tomllib.loads(value['after'])
own=value['workspace'].replace('/','\\').lower()
for key in list(after.get('projects',{})):
 if key.replace('/','\\').lower()==own and key not in before.get('projects',{}):
  assert after['projects'][key]=={'trust_level':'trusted'}, 'Unexpected own-project trust value'
  del after['projects'][key]
assert before==after, 'Native consumer changed settings beyond its own directory trust'
print('preserved')
'@
    $payload = @{before=$configBefore;after=(Get-Content -LiteralPath $configPath -Raw);workspace=$workspace} | ConvertTo-Json -Compress
    $oldOutputEncoding = $OutputEncoding
    try {
        $OutputEncoding = $utf8
        $comparison = $payload | & $python -B -c $compareScript
        if ($LASTEXITCODE -ne 0 -or $comparison -ne 'preserved') { throw 'Foreign native TOML settings changed; preserve them for review.' }
    } finally { $OutputEncoding = $oldOutputEncoding }
}
function Get-McpLspDescendants([int]$ParentId) {
    $snapshot = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate)
    $parents = [Collections.Generic.HashSet[int]]::new()
    $null = $parents.Add($ParentId)
    do {
        $added = $false
        foreach ($process in $snapshot) {
            if ($parents.Contains([int]$process.ParentProcessId) -and $parents.Add([int]$process.ProcessId)) { $added = $true }
        }
    } while ($added)
    return @($snapshot | Where-Object { $_.ProcessId -ne $ParentId -and $parents.Contains([int]$_.ProcessId) })
}
function Remove-McpLsp([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full -ne $ProbeRoot -and -not $full.StartsWith($ProbeRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped owned fixture.' }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full, $false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-McpLsp $child.FullName }
        [IO.Directory]::Delete($full, $false)
    } else { [IO.File]::Delete($full) }
}
try {
    $null = New-Item -ItemType Directory -Path (Join-Path $workspace '.serena') -Force
    Write-McpLsp (Join-Path $workspace '.serena/project.yml') "project_name: 'native-mcp-$([guid]::NewGuid().ToString('N'))'`nlanguages: [typescript]`nencoding: utf-8`n"
    Write-McpLsp (Join-Path $workspace 'index.ts') "export const value: number = 1;`n"
    Write-McpLsp (Join-Path $workspace 'tsconfig.json') '{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}'
    Write-Output "Outside-project MCP acceptance: $ProbeRoot"
    $server = Start-ConsumerServer $NativeCodex $CodexHome $workspace
    $listed = Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    $hooks = @($listed.data | ForEach-Object { $_.hooks })
    Assert-McpLsp ($hooks.Count -ge 7 -and @($hooks | Where-Object trustStatus -NE 'trusted').Count -eq 0) 'fresh native consumer loads seven trusted global hooks'
    if (-not $RunAgent) {
        Write-Output 'Prepared native global fixture only; -RunAgent is required for actual MCP edit acceptance.'
        $succeeded = $true
        return
    }
    # A normal persisted thread gives the exact native transcript pathname. It
    # identifies only this session's diagnostic directory; no unrelated report
    # or transcript is read, and no diagnostics tool is invoked by the harness.
    $thread = Invoke-ConsumerRpc $server 'thread/start' @{cwd=$workspace;ephemeral=$false}
    Assert-McpLsp ([bool]$thread.thread.path) 'native thread supplies its own transcript identity'
    $identity = @{workspace=$workspace;session_id=$thread.thread.id;transcript_path=$thread.thread.path;codex_home=$CodexHome}
    $identityPath = Join-Path $ProbeRoot 'private-thread-identity.json'
    Write-McpLsp $identityPath ($identity | ConvertTo-Json -Compress)
    $locate = @'
import json, os, pathlib, sys
sys.path.insert(0, str(pathlib.Path(sys.argv[1]) / 'tools/lsp'))
event = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding='utf-8'))
os.environ['CODEX_HOME'] = event.pop('codex_home')
from journal import state_directory
print(state_directory(event))
'@
    $runtime = (& $python -B -c $locate $repository $identityPath).Trim()
    if ($LASTEXITCODE -ne 0) { throw 'Could not derive this native thread diagnostic identity.' }
    $prompt = @"
This is an authorized disposable integration fixture at $workspace. Use the already connected global Serena MCP tools. Do not add or change any MCP configuration.
Read Serena initial_instructions if needed, then call activate_project with the exact absolute fixture path above. Use exactly two separate successful replace_in_files calls to edit only index.ts. Do not run a shell, apply_patch, another editing tool, any diagnostics tool, or read diagnostic report files. Metadata discovery is allowed if Serena tools are not exposed yet.
For the first replacement use relative_path='index.ts', mode='literal', expected_count=1, dry_run=false. Replace the exact line export const value: number = 1; with export const value: number = "wrong";.
Read the AUTOMATIC hook feedback before proceeding. Then use a second separate replace_in_files call with the same guards to replace export const value: number = "wrong"; with export const value: number = 2;.
Read its AUTOMATIC hook feedback. Report the original Serena replacement result from each call and the actual hook diagnostic status and code. For each edit also quote the first 12 characters of its revision hash from the Automatic language diagnostics feedback. These revision hashes are essential evidence; do not calculate, guess or invent them. If automatic feedback is absent, report that fact instead of claiming verification. Include the actual clearance status for the corrected revision.
"@
    $turn = Invoke-ConsumerRpc $server 'turn/start' @{threadId=$thread.thread.id;input=@(@{type='text';text=$prompt})}
    $events = [Collections.Generic.List[object]]::new()
    $deadline = [DateTime]::UtcNow.AddSeconds(240)
    $finished = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        $read = $server.Process.StandardOutput.ReadLineAsync()
        if (-not $read.Wait([Math]::Max(1, [int]($deadline-[DateTime]::UtcNow).TotalMilliseconds))) { throw 'Native Serena model turn timed out.' }
        if ($null -eq $read.Result) { throw 'Native consumer exited before the MCP acceptance turn completed.' }
        $event = $read.Result | ConvertFrom-Json -AsHashtable
        $events.Add($event)
        [IO.File]::AppendAllText((Join-Path $ProbeRoot 'private-agent-events.jsonl'), $read.Result + "`n", $utf8)
        if ($event.ContainsKey('method') -and $event.method -eq 'turn/completed' -and $event.params.turn.id -eq $turn.turn.id) { $finished=$true; break }
    }
    Assert-McpLsp $finished 'real global native model turn finishes'
    Write-McpLsp (Join-Path $ProbeRoot 'private-agent-events.json') ($events | ConvertTo-Json -Depth 70)
    $items = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' } | ForEach-Object { $_.params.item })
    $calls = @($items | Where-Object { $_.type -eq 'mcpToolCall' })
    $activations = @($calls | Where-Object { $_['server'] -eq 'serena' -and $_['tool'] -eq 'activate_project' })
    $edits = @($calls | Where-Object { $_['server'] -eq 'serena' -and $_['tool'] -eq 'replace_in_files' })
    Assert-McpLsp ($activations.Count -eq 1 -and $activations[0].arguments.project -eq $workspace) 'Serena actually activates the exact outside fixture'
    Assert-McpLsp ($edits.Count -eq 2 -and @($edits | Where-Object { $_['status'] -ne 'completed' -or $_['error'] -or $_['result']['isError'] }).Count -eq 0) 'both original Serena MCP edit results remain successful'
    Assert-McpLsp (@($items | Where-Object { $_.type -in @('commandExecution','fileChange') }).Count -eq 0) 'the model uses no shell or native patch to create the tested edits'
    Assert-McpLsp (@($calls | Where-Object { $_['tool'] -match 'diagnos|diagnostic' }).Count -eq 0) 'the model does not invoke diagnostics manually'
    $reports = @(Get-ChildItem -LiteralPath $runtime -Filter 'report-*.json' -File | ForEach-Object { Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json })
    Write-McpLsp (Join-Path $ProbeRoot 'scoped-diagnostics.json') ($reports | ConvertTo-Json -Depth 50)
    Assert-McpLsp ($reports.Count -ge 2 -and @($reports | Where-Object { $_.workspace -ne $workspace -or $_.session_id -ne $thread.thread.id }).Count -eq 0) 'diagnostic reports belong only to this workspace and native session'
    $postReports = @($reports | Where-Object { $_.event -eq 'PostToolUse' -and $_.tool_name -match 'serena.*replace_in_files$' })
    $results = @($postReports | ForEach-Object { $_.results } | Where-Object file -EQ 'index.ts')
    $bad = @($results | Where-Object { $_.status -eq 'diagnostics' -and @($_.diagnostics | Where-Object { [string]$_.code -eq '2322' }).Count -gt 0 })
    $good = @($results | Where-Object { $_.status -eq 'clean' -and $_.diagnostics.Count -eq 0 })
    Assert-McpLsp ($bad.Count -ge 1) 'native PostToolUse after Serena edit produces TypeScript code 2322'
    Assert-McpLsp ($good.Count -ge 1) 'separate Serena correction receives an authoritative clean diagnostic set'
    Assert-McpLsp ($bad[0].revision -ne $good[-1].revision) 'error and clearance retain distinct source revisions'
    $messages = @($items | Where-Object type -EQ 'agentMessage' | ForEach-Object { $_.text }) -join "`n"
    Assert-McpLsp ($messages.Contains('2322') -and $messages -match 'clean' -and $messages.Contains($bad[0].revision.Substring(0,12)) -and $messages.Contains($good[-1].revision.Substring(0,12))) 'the model quotes both hook-only revision hashes and actual error/clearance feedback'
    Assert-McpLsp ((Get-Content -LiteralPath (Join-Path $workspace 'index.ts') -Raw).Trim() -eq 'export const value: number = 2;') 'the intended corrected source remains on disk'
    Assert-PreservedMcpLsp
    Assert-McpLsp $true 'shared Serena config, hook source and registrations retain exact bytes; native TOML adds only its own directory trust'
    $report = @{status='passed';checks=$checks;assertions=$checks.Count;nativeVersion=(& $NativeCodex --version);threadId=$thread.thread.id;
        workspace=$workspace;globalHome=$CodexHome;modelProbe=$true;errorCode='2322';errorRevision=$bad[0].revision;cleanRevision=$good[-1].revision;manualDiagnostics=$false}
    Write-McpLsp (Join-Path $ProbeRoot 'report.json') ($report | ConvertTo-Json -Depth 10)
    Write-Output ($report | ConvertTo-Json -Depth 10)
    $succeeded = $true
} catch {
    $failure = $_
    if (Test-Path -LiteralPath $ProbeRoot) {
        Write-McpLsp (Join-Path $ProbeRoot 'failure.json') (@{status='failed';reason=$_.Exception.Message;checks=$checks} | ConvertTo-Json -Depth 10)
    }
    throw
} finally {
    if ($server) {
        $ownedProcesses = @(Get-McpLspDescendants $server.Process.Id)
        Stop-ConsumerServer $server
        $cleanupDeadline = [DateTime]::UtcNow.AddSeconds(5)
        do {
            $remaining = @(Get-CimInstance Win32_Process | Where-Object {
                $observed = $_
                @($ownedProcesses | Where-Object { $_.ProcessId -eq $observed.ProcessId -and $_.CreationDate -eq $observed.CreationDate }).Count -gt 0
            } | Select-Object ProcessId,CreationDate)
            if ($remaining.Count -eq 0) { break }
            Start-Sleep -Milliseconds 200
        } while ([DateTime]::UtcNow -lt $cleanupDeadline)
        if ($remaining.Count) {
            $succeeded = $false
            Write-McpLsp (Join-Path $ProbeRoot 'cleanup-pending.json') ($remaining | ConvertTo-Json -Depth 5)
            Write-McpLsp (Join-Path $ProbeRoot 'report.json') (@{status='cleanup-pending';checks=$checks;assertions=$checks.Count} | ConvertTo-Json -Depth 10)
            if (-not $failure) { throw 'Owned MCP descendants remain after native shutdown; retained exact process identities for review.' }
            Write-Warning 'Owned MCP descendants remain after native shutdown; process identities were retained for review.'
        } elseif ($RunAgent) { Write-Output 'PASS: native shutdown leaves no observed owned MCP descendant process' }
    }
    try { Assert-PreservedMcpLsp } catch {
        $succeeded = $false
        if ($failure) { Write-Warning $_.Exception.Message } else { throw }
    }
    if (Test-Path -LiteralPath $ProbeRoot) {
        if ($KeepProbe -or -not $succeeded) { Write-Output "Retained private MCP acceptance evidence: $ProbeRoot" }
        else { Remove-McpLsp $ProbeRoot }
    }
}
