#Requires -Version 7.4
<#!
Actual adopted Serena MCP in two disposable projects. No model, installs,
updates, global registration or upstream edits. A registry is required.
#>
param(
    [Parameter(Mandatory)][string]$Registry,
    [switch]$KeepProbe
)
$ErrorActionPreference = 'Stop'
$sourceRoot = Split-Path $PSScriptRoot -Parent
$inventory = Get-Content -LiteralPath $Registry -Raw | ConvertFrom-Json -AsHashtable
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
$entry = Join-Path $sourceRoot 'tools/code-tools/serena_entry.py'
$probe = Join-Path ([IO.Path]::GetTempPath()) ('harness-serena-' + [guid]::NewGuid().ToString('N'))
$config = Join-Path $env:USERPROFILE '.serena/serena_config.yml'
$configHash = if (Test-Path -LiteralPath $config) { (Get-FileHash -LiteralPath $config).Hash } else { $null }
$dependencyHashes = @{}
foreach ($language in $inventory.languages) {
    foreach ($path in @($language.paths.executable, $language.paths.entrypoint) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) }) {
        $dependencyHashes[$path] = (Get-FileHash -LiteralPath $path).Hash
    }
}
$servers = @()
$script:assertions = 0
function Assert-Serena([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
    $script:assertions++
    Write-Host "PASS $Message"
}
function Invoke-SerenaRpc($Server, [string]$Method, [hashtable]$Parameters, [int]$TimeoutSeconds = 60) {
    $id = $Server.NextId++
    $request = @{jsonrpc='2.0';id=$id;method=$Method;params=$Parameters} | ConvertTo-Json -Depth 40 -Compress
    $Server.Process.StandardInput.WriteLine($request)
    $Server.Process.StandardInput.Flush()
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $read = $Server.Process.StandardOutput.ReadLineAsync()
        if (-not $read.Wait([Math]::Max(1, [int]($deadline - [DateTime]::UtcNow).TotalMilliseconds))) { throw "Serena timed out: $Method" }
        if ($null -eq $read.Result) { throw "Serena exited: $Method; $($Server.Errors.GetAwaiter().GetResult())" }
        $message = $read.Result | ConvertFrom-Json -AsHashtable
        if ($message.ContainsKey('id') -and $message.id -eq $id) {
            if ($message.ContainsKey('error')) { throw ($message.error | ConvertTo-Json -Depth 20 -Compress) }
            return $message.result
        }
    }
    throw "Serena timed out: $Method"
}
function Invoke-SerenaTool($Server, [string]$Name, [hashtable]$Arguments) {
    $result = Invoke-SerenaRpc $Server 'tools/call' @{name=$Name;arguments=$Arguments}
    $text = ($result.content | Where-Object type -EQ 'text' | ForEach-Object text) -join "`n"
    if ($result.isError -or $text.StartsWith('Error:')) { throw "Serena ${Name}: $text" }
    return $text
}
function Start-Serena([string]$Workspace) {
    $info = [Diagnostics.ProcessStartInfo]::new($python)
    foreach ($argument in @('-B','-u',$entry,'start-mcp-server','--context','codex','--project-from-cwd','--enable-web-dashboard','false','--enable-gui-log-window','false')) { $info.ArgumentList.Add($argument) }
    $info.WorkingDirectory = $Workspace
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.Environment['HARNESS_CODE_TOOLS_REGISTRY'] = [IO.Path]::GetFullPath($Registry)
    $info.Environment['PYTHONUTF8'] = '1'
    $process = [Diagnostics.Process]::Start($info)
    $server = @{Process=$process;NextId=1;Errors=$process.StandardError.ReadToEndAsync()}
    $script:servers += $server
    $hello = Invoke-SerenaRpc $server 'initialize' @{protocolVersion='2024-11-05';capabilities=@{};clientInfo=@{name='harness-serena-tests';version='1'}}
    Assert-Serena ($hello.serverInfo.name -eq 'Serena') 'real Serena MCP handshake'
    $process.StandardInput.WriteLine('{"jsonrpc":"2.0","method":"notifications/initialized"}')
    $process.StandardInput.Flush()
    return $server
}
function Remove-OwnedTree([string]$Path) {
    $absolute = [IO.Path]::GetFullPath($Path)
    $root = [IO.Path]::GetFullPath($probe)
    if ($absolute -ne $root -and -not $absolute.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped owned probe' }
    $item = Get-Item -LiteralPath $absolute -Force -ErrorAction SilentlyContinue
    if ($null -eq $item) { return }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { $item.Delete(); return }
    if ($item.PSIsContainer) { foreach ($child in Get-ChildItem -LiteralPath $absolute -Force) { Remove-OwnedTree $child.FullName } }
    Remove-Item -LiteralPath $absolute -Force
}
try {
    $null = New-Item -ItemType Directory -Path $probe
    & $python -B (Join-Path $PSScriptRoot 'fixtures/code-tools-native/serena-guard-check.py') $entry $Registry $probe
    Assert-Serena ($LASTEXITCODE -eq 0) 'runtime provisioning guards reject missing dependencies before mutation'
    $runtime = Join-Path $probe 'guard-codex-home/harness/runtime/serena'
    Assert-Serena (@(Get-ChildItem -LiteralPath $runtime -Force).Count -eq 0) 'process-owned PowerShell temp paths are distinct and cleaned after exit'
    foreach ($name in @('проект alpha','project beta')) {
        $workspace = Join-Path $probe $name
        $null = New-Item -ItemType Directory -Path (Join-Path $workspace '.serena') -Force
        $marker = if ($name.Contains('alpha')) { 101 } else { 202 }
        [IO.File]::WriteAllText((Join-Path $workspace '.serena/project.yml'), "project_name: '$name'`nlanguages: [typescript]`nencoding: utf-8`n")
        [IO.File]::WriteAllText((Join-Path $workspace 'tsconfig.json'), '{"compilerOptions":{"strict":true,"noEmit":true,"target":"ES2022"},"include":["*.ts"]}')
        [IO.File]::WriteAllText((Join-Path $workspace 'main.ts'), "export function shared(): number { return $marker; }`nexport const localUse = shared();`n")
        $null = Start-Serena $workspace
    }
    $a,$b = $servers
    $tools = Invoke-SerenaRpc $a 'tools/list' @{}
    Assert-Serena (@($tools.tools.name) -contains 'get_diagnostics_for_file') 'native diagnostics tool exposed in Codex context'
    Assert-Serena (@($tools.tools.name) -notcontains 'execute_shell_command') 'Codex context excludes general shell tool'
    foreach ($server in @($a,$b)) {
        $overview = Invoke-SerenaTool $server 'get_symbols_overview' @{relative_path='main.ts'}
        Assert-Serena ($overview.Contains('shared') -and $overview.Contains('localUse')) 'document symbols from active root'
        $references = Invoke-SerenaTool $server 'find_referencing_symbols' @{relative_path='main.ts';name_path='shared'}
        Assert-Serena ($references.Contains('localUse')) 'reference search finds actual caller'
        $clean = Invoke-SerenaTool $server 'get_diagnostics_for_file' @{relative_path='main.ts'}
        Assert-Serena ($clean.Trim() -eq '{}') 'initial diagnostics are explicitly empty'
    }
    $readA = Invoke-SerenaTool $a 'find_symbol' @{relative_path='main.ts';name_path_pattern='shared';include_body=$true}
    $readB = Invoke-SerenaTool $b 'find_symbol' @{relative_path='main.ts';name_path_pattern='shared';include_body=$true}
    Assert-Serena ($readA.Contains('return 101;') -and -not $readA.Contains('return 202;')) 'same-name symbol read isolates alpha root'
    Assert-Serena ($readB.Contains('return 202;') -and -not $readB.Contains('return 101;')) 'same-name symbol read isolates beta root'
    $edit = Invoke-SerenaTool $a 'replace_symbol_body' @{relative_path='main.ts';name_path='shared';body='export function shared(): number { return "wrong"; }'}
    Assert-Serena ($edit.Contains('OK')) 'real bounded semantic edit completes'
    $diagnostics = Invoke-SerenaTool $a 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($diagnostics.Contains('2322') -or $diagnostics.Contains("not assignable to type 'number'")) 'semantic edit produces actual TypeScript type error'
    $cleanB = Invoke-SerenaTool $b 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($cleanB.Trim() -eq '{}') 'alpha diagnostics do not contaminate concurrent beta root'
    $unchangedB = Invoke-SerenaTool $b 'find_symbol' @{relative_path='main.ts';name_path_pattern='shared';include_body=$true}
    Assert-Serena ($unchangedB.Contains('return 202;')) 'alpha semantic edit leaves beta bytes unchanged'
    $null = Invoke-SerenaTool $a 'replace_symbol_body' @{relative_path='main.ts';name_path='shared';body='export function shared(): number { return 101; }'}
    $fixed = Invoke-SerenaTool $a 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($fixed.Trim() -eq '{}') 'correcting semantic edit clears diagnostics'
    $null = Invoke-SerenaTool $b 'replace_symbol_body' @{relative_path='main.ts';name_path='shared';body='export function shared(): number { return "beta wrong"; }'}
    $badB = Invoke-SerenaTool $b 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($badB.Contains('2322') -or $badB.Contains("not assignable to type 'number'")) 'second root semantic edit yields its own type error'
    $stillCleanA = Invoke-SerenaTool $a 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($stillCleanA.Trim() -eq '{}') 'second root error leaves first root clean'
    $null = Invoke-SerenaTool $b 'replace_symbol_body' @{relative_path='main.ts';name_path='shared';body='export function shared(): number { return 202; }'}
    $fixedB = Invoke-SerenaTool $b 'get_diagnostics_for_file' @{relative_path='main.ts'}
    Assert-Serena ($fixedB.Trim() -eq '{}') 'second root correction clears its diagnostics'
    $a.Process.StandardInput.Close()
    if (-not $a.Process.WaitForExit(3000)) { $a.Process.Kill($true); $a.Process.WaitForExit() }
    $survivor = Invoke-SerenaTool $b 'find_symbol' @{relative_path='main.ts';name_path_pattern='shared';include_body=$true}
    Assert-Serena ($survivor.Contains('return 202;')) 'one consumer shutdown preserves the other server'
    $finalHash = if (Test-Path -LiteralPath $config) { (Get-FileHash -LiteralPath $config).Hash } else { $null }
    Assert-Serena ($configHash -eq $finalHash) 'shared Serena configuration byte-for-byte preserved'
    foreach ($path in $dependencyHashes.Keys) {
        Assert-Serena ((Get-FileHash -LiteralPath $path).Hash -eq $dependencyHashes[$path]) "adopted dependency unchanged: $([IO.Path]::GetFileName($path))"
    }
    Write-Host "Serena MCP checks passed: $script:assertions assertions."
}
finally {
    foreach ($server in $servers) {
        if (-not $server.Process.HasExited) {
            $server.Process.StandardInput.Close()
            if (-not $server.Process.WaitForExit(3000)) { $server.Process.Kill($true); $server.Process.WaitForExit() }
        }
        if ($KeepProbe) { [IO.File]::WriteAllText((Join-Path $probe "serena-$($server.Process.Id).stderr.log"), $server.Errors.GetAwaiter().GetResult()) }
        $server.Process.Dispose()
    }
    if ($KeepProbe) { Write-Host "Retained disposable probe: $probe" } else { Remove-OwnedTree $probe }
}
