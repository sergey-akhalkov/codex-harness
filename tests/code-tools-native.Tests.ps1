#requires -Version 7.4
# Isolated native Codex contract checks. No upstream packages or global settings change.
# -RunAgent opts into bounded model-backed proof. -InspectTrust stops at the native UI.
[CmdletBinding()]
param(
    [string]$NativeCodex,
    [string]$AuthSource,
    [string]$ProbeRoot,
    [switch]$RunAgent,
    [switch]$InspectTrust,
    [switch]$KeepProbe
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
$repository = Split-Path -Parent $PSScriptRoot
$hostHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
if (-not $NativeCodex) {
    $registration = Get-Content -LiteralPath (Join-Path $hostHome 'harness/installation.json') -Raw | ConvertFrom-Json
    $vendor = Join-Path (Split-Path -Parent $registration.codexCommand) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
    $NativeCodex = (Get-ChildItem -LiteralPath $vendor -Recurse -Filter codex.exe | Select-Object -First 1).FullName
}
if (-not $AuthSource) { $AuthSource = Join-Path $hostHome 'auth.json' }
if (-not $ProbeRoot) { $ProbeRoot = Join-Path ([IO.Path]::GetTempPath()) ('code-tools-native-' + [guid]::NewGuid().ToString('N')) }
$ProbeRoot = [IO.Path]::GetFullPath($ProbeRoot)
$codexHome = Join-Path $ProbeRoot 'codex home'
$workspace = Join-Path $ProbeRoot 'workspace with spaces'
$source = Join-Path $PSScriptRoot ('fixtures/code-tools-native/.probe-' + [guid]::NewGuid().ToString('N'))
$tracePath = Join-Path $ProbeRoot 'mcp-trace.jsonl'
$markerPath = Join-Path $source 'source-marker.txt'
$hooksSource = Join-Path $source 'hooks.json'
$server = $null
$terminal = $null
$oldCodexHome = $env:CODEX_HOME
$utf8 = [Text.UTF8Encoding]::new($false)
$checks = [Collections.Generic.List[string]]::new()
function Assert-Native([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $checks.Add($Message)
    Write-Output "PASS: $Message"
}
function Write-Probe([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Invoke-Native([string[]]$Arguments, [int]$TimeoutSeconds = 30) {
    $start = [Diagnostics.ProcessStartInfo]::new($NativeCodex)
    foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $workspace
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.Environment['CODEX_HOME'] = $codexHome
    $process = [Diagnostics.Process]::Start($start)
    $process.StandardInput.Close()
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    try {
        if (-not $process.WaitForExit($TimeoutSeconds * 1000)) { $process.Kill($true); throw 'Native probe timeout.' }
        return @{ exitCode = $process.ExitCode; stdout = $stdout.GetAwaiter().GetResult(); stderr = $stderr.GetAwaiter().GetResult() }
    } finally { $process.Dispose() }
}
function Remove-ProbeTree([string]$Path, [string]$AllowedRoot = $ProbeRoot) {
    $full = [IO.Path]::GetFullPath($Path)
    $allowed = [IO.Path]::GetFullPath($AllowedRoot)
    if ($full -ne $allowed -and -not $full.StartsWith($allowed + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped probe.' }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full, $false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-ProbeTree $child.FullName $allowed }
        [IO.Directory]::Delete($full, $false)
    } else { [IO.File]::Delete($full) }
}
try {
    if (Test-Path -LiteralPath (Join-Path $codexHome 'config.toml')) { throw 'Refusing to overwrite an existing probe configuration.' }
    foreach ($directory in @($codexHome, $workspace, $source)) { $null = New-Item -ItemType Directory -Path $directory -Force }
    Write-Output "Probe: $ProbeRoot"
    Write-Probe $markerPath 'SOURCE_VERSION_ONE'
    Write-Probe $tracePath ''
    $workspaceToml = ConvertTo-Json $workspace -Compress
    Write-Probe (Join-Path $codexHome 'config.toml') @"
model = "gpt-6-astra"
model_reasoning_effort = "low"
approval_policy = "never"
sandbox_mode = "danger-full-access"
developer_instructions = "NATIVE_UNRELATED_BASE_SENTINEL"
[windows]
sandbox = "unelevated"
[projects.$workspaceToml]
trust_level = "trusted"
"@
    if ((Test-Path -LiteralPath $AuthSource) -and -not (Test-Path -LiteralPath (Join-Path $codexHome 'auth.json'))) {
        $null = New-Item -ItemType SymbolicLink -Path (Join-Path $codexHome 'auth.json') -Target $AuthSource
    }
    $node = (Get-Command node -CommandType Application | Select-Object -First 1).Source
    $fixture = Join-Path $PSScriptRoot 'fixtures/code-tools-native/mcp-fixture.mjs'
    $registered = Invoke-Native @('mcp', 'add', 'native-contract', '--', $node, $fixture, $workspace, $tracePath, $markerPath)
    Assert-Native ($registered.exitCode -eq 0) 'native MCP path registration succeeds'
    Assert-Native ((Get-Content -LiteralPath (Join-Path $codexHome 'config.toml') -Raw).Contains('NATIVE_UNRELATED_BASE_SENTINEL')) 'native MCP registration preserves unrelated base setting'
    $hook = @{ hooks = @{ PostToolUse = @(@{ matcher = 'apply_patch|Bash|mcp__.*'; hooks = @(@{
        type = 'mcp_tool'; server = 'native-contract'; tool = 'diagnostics_after_tool'; timeout = 30; statusMessage = 'Native contract diagnostic check';
        input = @{ workspace = '${cwd}'; session_id = '${session_id}'; turn_id = '${turn_id}'; tool_use_id = '${tool_use_id}'; tool_name = '${tool_name}'; tool_input = '${tool_input}'; tool_response = '${tool_response}' }
    }) }) } }
    Write-Probe $hooksSource ($hook | ConvertTo-Json -Depth 20)
    $hookLink = Join-Path $codexHome 'hooks.json'
    if (-not (Test-Path -LiteralPath $hookLink)) { $null = New-Item -ItemType SymbolicLink -Path $hookLink -Target $hooksSource }
    $server = Start-ConsumerServer $NativeCodex $codexHome $workspace
    $listed = Invoke-ConsumerRpc $server 'hooks/list' @{ cwds = @($workspace) }
    Write-Probe (Join-Path $ProbeRoot 'hooks-before.json') ($listed | ConvertTo-Json -Depth 30)
    Assert-Native ($listed.data[0].hooks.Count -eq 1 -and $listed.data[0].hooks[0].handlerType -eq 'mcpTool') 'unprofiled app-server discovers linked MCP hook definition'
    Assert-Native ($listed.data[0].hooks[0].trustStatus -eq 'untrusted') 'new exact hook starts untrusted'
    Stop-ConsumerServer $server; $server = $null
    . (Join-Path $PSScriptRoot 'native-hook-trust.ps1')
    $nativeTrust = Invoke-NativeHookTrust -NativeCodex $NativeCodex -CodexHome $codexHome -Workspace $workspace -TracePath (Join-Path $ProbeRoot 'private-terminal.txt')
    if ($InspectTrust) {
        Write-Output "Native trust confirmed for $($nativeTrust.TrustedCount) hooks. Model delivery not exercised."
        return
    }
    Assert-Native ($nativeTrust.ExitCode -eq 0) 'native TUI reviews and trusts the exact hook without a bypass'
    $server = Start-ConsumerServer $NativeCodex $codexHome $workspace
    $trusted = Invoke-ConsumerRpc $server 'hooks/list' @{ cwds = @($workspace) }
    Assert-Native ($trusted.data[0].hooks[0].trustStatus -eq 'trusted') 'fresh unprofiled consumer reads native persisted hook trust'
    Assert-Native ((Get-Item -LiteralPath $hookLink -Force).LinkType -eq 'SymbolicLink') 'native trust leaves the source link intact'
    $thread = Invoke-ConsumerRpc $server 'thread/start' @{ cwd = $workspace; ephemeral = $true }
    $identity = Invoke-ConsumerRpc $server 'mcpServer/tool/call' @{threadId=$thread.thread.id;server='native-contract';tool='identity';arguments=@{}}
    Write-Probe (Join-Path $ProbeRoot 'identity-first.json') ($identity | ConvertTo-Json -Depth 30)
    Assert-Native (($identity | ConvertTo-Json -Depth 30 -Compress).Contains('SOURCE_VERSION_ONE')) 'unprofiled consumer calls MCP from its repository source path'
    Stop-ConsumerServer $server; $server = $null
    Write-Probe $markerPath 'SOURCE_VERSION_TWO'
    $server = Start-ConsumerServer $NativeCodex $codexHome $workspace
    $thread = Invoke-ConsumerRpc $server 'thread/start' @{ cwd = $workspace; ephemeral = $true }
    $identity = Invoke-ConsumerRpc $server 'mcpServer/tool/call' @{threadId=$thread.thread.id;server='native-contract';tool='identity';arguments=@{}}
    Assert-Native (($identity | ConvertTo-Json -Depth 30 -Compress).Contains('SOURCE_VERSION_TWO')) 'new consumer sees the changed checkout source without reinstall or copied deployment'
    if ($RunAgent) {
        $prompt = 'Use the native apply_patch tool to create hello.txt containing exactly hello. Do not call diagnostic tools manually. After the edit, give a concise final answer including any verification marker supplied automatically by tool feedback.'
        $turn = Invoke-ConsumerRpc $server 'turn/start' @{threadId=$thread.thread.id;input=@(@{type='text';text=$prompt})}
        $events = [Collections.Generic.List[object]]::new()
        $deadline = [DateTime]::UtcNow.AddSeconds(120)
        $finished = $false
        while ([DateTime]::UtcNow -lt $deadline) {
            $read = $server.Process.StandardOutput.ReadLineAsync()
            if (-not $read.Wait([Math]::Max(1, [int]($deadline-[DateTime]::UtcNow).TotalMilliseconds))) { throw 'Native agent turn timed out.' }
            if ($null -eq $read.Result) { throw 'Native app-server exited during agent turn.' }
            $event = $read.Result | ConvertFrom-Json -AsHashtable
            $events.Add($event)
            if ($event.ContainsKey('method') -and $event.method -eq 'turn/completed' -and $event.params.turn.id -eq $turn.turn.id) { $finished = $true; break }
        }
        Write-Probe (Join-Path $ProbeRoot 'agent-events.json') ($events | ConvertTo-Json -Depth 60)
        Assert-Native $finished 'native model-backed edit turn reaches completion'
        $trace = @(Get-Content -LiteralPath $tracePath | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
        $hookCalls = @($trace | Where-Object { $_.ContainsKey('name') -and $_.name -eq 'diagnostics_after_tool' })
        $nonces = @($trace | Where-Object { $_.ContainsKey('hookNonce') } | ForEach-Object { $_.hookNonce })
        Assert-Native ($hookCalls.Count -eq 1 -and $hookCalls[0].input.tool_name -eq 'apply_patch') 'actual native apply_patch automatically invokes exactly one MCP hook without recursion'
        $final = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' -and $_.params.item.type -eq 'agentMessage' } | ForEach-Object { $_.params.item.text }) -join "`n"
        Assert-Native ($nonces.Count -eq 1 -and $final.Contains($nonces[0])) 'the model receives the hook-only nonce through additionalContext'
        Assert-Native ((Get-Content -LiteralPath (Join-Path $workspace 'hello.txt') -Raw).Trim() -eq 'hello') 'native edit contents survive the hook'
        $fileChanges = @($events | Where-Object { $_.ContainsKey('method') -and $_.method -eq 'item/completed' -and $_.params.item.type -eq 'fileChange' })
        Assert-Native ($fileChanges.Count -eq 1 -and $fileChanges[0].params.item.status -eq 'completed') 'the original native patch result remains completed and visible'
        Stop-ConsumerServer $server; $server = $null
        Write-Probe $tracePath ''
        $cli = Invoke-Native @('exec', '--skip-git-repo-check', '--json', 'Use the native-contract MCP write_note tool once with text cli-change. Do not call diagnostic tools manually or run shell commands. Report the original tool result and any verification marker supplied automatically by tool feedback.') 120
        Write-Probe (Join-Path $ProbeRoot 'cli-events.jsonl') $cli.stdout
        Write-Probe (Join-Path $ProbeRoot 'cli-stderr.txt') $cli.stderr
        Assert-Native ($cli.exitCode -eq 0) 'ordinary unprofiled native CLI exec completes its MCP edit'
        $cliTrace = @(Get-Content -LiteralPath $tracePath | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
        $cliHooks = @($cliTrace | Where-Object { $_.ContainsKey('name') -and $_.name -eq 'diagnostics_after_tool' })
        $cliNonces = @($cliTrace | Where-Object { $_.ContainsKey('hookNonce') } | ForEach-Object { $_.hookNonce })
        Assert-Native ($cliHooks.Count -eq 1 -and $cliHooks[0].input.tool_name -match '^mcp__.*write_note$') 'native CLI MCP mutation automatically triggers one non-recursive hook'
        $cliEvents = @($cli.stdout -split '\r?\n' | Where-Object { $_.Trim() } | ForEach-Object { $_ | ConvertFrom-Json -AsHashtable })
        $cliFinal = @($cliEvents | Where-Object { $_.type -eq 'item.completed' -and $_.item.type -eq 'agent_message' } | ForEach-Object { $_.item.text }) -join "`n"
        $originalCalls = @($cliEvents | Where-Object { $_.type -eq 'item.completed' -and $_.item.type -eq 'mcp_tool_call' })
        Assert-Native ($cliNonces.Count -eq 1 -and $cliFinal.Contains($cliNonces[0])) 'native CLI agent sees the automatically supplied hook nonce'
        Assert-Native ($originalCalls.Count -eq 1 -and $originalCalls[0].item.status -eq 'completed' -and ($originalCalls[0].item.result | ConvertTo-Json -Depth 10 -Compress).Contains('NATIVE_ORIGINAL_TOOL_RESULT') -and $cliFinal.Contains('SOURCE_VERSION_TWO')) 'native CLI retains original MCP result and reads the current checkout source'
        Assert-Native ((Get-Content -LiteralPath (Join-Path $workspace 'native-note.txt') -Raw) -eq 'cli-change') 'native CLI MCP write remains intact'
    }
    if ($null -ne $server) { Stop-ConsumerServer $server; $server = $null }
    $hook.hooks.PostToolUse[0].hooks[0].statusMessage = 'Changed source requires new native trust'
    Write-Probe $hooksSource ($hook | ConvertTo-Json -Depth 20)
    $server = Start-ConsumerServer $NativeCodex $codexHome $workspace
    $changed = Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($workspace)}
    Assert-Native ($changed.data[0].hooks[0].trustStatus -eq 'modified' -and $changed.data[0].hooks[0].statusMessage -eq 'Changed source requires new native trust') 'new consumer reads edited hook source directly and invalidates stale trust'
    $report = @{nativeVersion=(& $NativeCodex --version);checks=$checks;modelProbe=[bool]$RunAgent;source=$source;probe=$ProbeRoot}
    Write-Probe (Join-Path $ProbeRoot 'report.json') ($report | ConvertTo-Json -Depth 20)
    Write-Output ($report | ConvertTo-Json -Depth 10)
} finally {
    $env:CODEX_HOME = $oldCodexHome
    if ($null -ne $terminal) { Write-Probe (Join-Path $ProbeRoot 'private-terminal.txt') $terminal.Transcript; $terminal.Dispose() }
    if ($null -ne $server) { Stop-ConsumerServer $server }
    if (Test-Path -LiteralPath $ProbeRoot) {
        if ($KeepProbe) { Write-Output "Retained isolated probe: $ProbeRoot" } else { Remove-ProbeTree $ProbeRoot }
    }
    if (Test-Path -LiteralPath $source) {
        if ($KeepProbe) { Write-Output "Retained test-owned source: $source" } else { Remove-ProbeTree $source $source }
    }
}
