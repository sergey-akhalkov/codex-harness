#requires -Version 7.4
# Actual source hook under a deliberately legacy Windows console encoding.
# Uses existing LSP installations and an isolated native-style CODEX_HOME only.
[CmdletBinding()]
param([string]$InventoryPath = (Join-Path $env:USERPROFILE '.codex/harness/code-tools.json'), [switch]$KeepProbe)
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath $InventoryPath -Raw | ConvertFrom-Json
$python = ($inventory.mcp | Where-Object id -EQ 'serena').paths.python
$root = Join-Path ([IO.Path]::GetTempPath()) ('harness-hook-utf8-' + [guid]::NewGuid().ToString('N'))
$codexPath = Join-Path $root 'codex'
$workspace = Join-Path $root 'проект с пробелами'
$utf8 = [Text.UTF8Encoding]::new($false)
$lf = [string][char]10
$checks = 0
$failed = $true
function Assert-HookEncoding([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw "FAIL: $Message" }
    $script:checks++
    Write-Output "PASS: $Message"
}
function Write-HookEncoding([string]$Path, [string]$Text) { [IO.File]::WriteAllText($Path, $Text, $utf8) }
function Invoke-LegacyHook([string]$Phase, [hashtable]$Payload) {
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Command pwsh -CommandType Application | Select-Object -First 1).Source)
    foreach ($argument in @('-NoLogo','-NoProfile','-File',(Join-Path $root 'legacy.ps1'),'-Entry',(Join-Path $repository 'tools/hook.ps1'),'-Phase',$Phase)) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory = $workspace
    $start.UseShellExecute = $false; $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true; $start.RedirectStandardOutput = $true; $start.RedirectStandardError = $true
    $start.StandardInputEncoding = $utf8; $start.StandardOutputEncoding = $utf8; $start.StandardErrorEncoding = $utf8
    $start.Environment['CODEX_HOME'] = $codexPath
    $process = [Diagnostics.Process]::Start($start)
    $output = $process.StandardOutput.ReadToEndAsync(); $errors = $process.StandardError.ReadToEndAsync()
    try {
        $process.StandardInput.WriteLine(($Payload | ConvertTo-Json -Depth 10 -Compress)); $process.StandardInput.Close()
        if (-not $process.WaitForExit(45000)) { $process.Kill($true); $process.WaitForExit(); throw 'Actual hook encoding probe timed out.' }
        if ($process.ExitCode) { throw ('Source hook failed: ' + $errors.GetAwaiter().GetResult()) }
        $text = $output.GetAwaiter().GetResult()
        Write-HookEncoding (Join-Path $root ($Phase + '-' + $Payload.tool_use_id + '.json')) $text
        return $text | ConvertFrom-Json
    } finally { $process.Dispose() }
}
function Remove-HookEncoding([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path)
    if ($full -ne $root -and -not $full.StartsWith($root + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped owned encoding fixture.' }
    $item = Get-Item -LiteralPath $full -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($item.PSIsContainer) { [IO.Directory]::Delete($full, $false) } else { [IO.File]::Delete($full) }
    } elseif ($item.PSIsContainer) {
        foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-HookEncoding $child.FullName }
        [IO.Directory]::Delete($full, $false)
    } else { [IO.File]::Delete($full) }
}
try {
    foreach ($directory in @($workspace,(Join-Path $codexPath 'harness'))) { $null = New-Item -ItemType Directory -Path $directory -Force }
    Write-HookEncoding (Join-Path $codexPath 'harness/installation.json') (@{sourceRoot=$repository} | ConvertTo-Json -Compress)
    Write-HookEncoding (Join-Path $codexPath 'harness/code-tools.json') ($inventory | ConvertTo-Json -Depth 60)
    $registry = & $python -B (Join-Path $repository 'tools/lsp/registry.py') --inventory $InventoryPath
    if ($LASTEXITCODE) { throw 'Pure LSP registry generation failed.' }
    Write-HookEncoding (Join-Path $codexPath 'harness/lsp-servers.json') ($registry -join $lf)
    Write-HookEncoding (Join-Path $workspace 'tsconfig.json') '{"compilerOptions":{"strict":true,"noEmit":true},"include":["*.ts"]}'
    Write-HookEncoding (Join-Path $workspace 'index.ts') ('export const value: number = 1;' + $lf)
    Write-HookEncoding (Join-Path $root 'legacy.ps1') @'
param([string]$Entry,[string]$Phase)
[Console]::InputEncoding = [Text.Encoding]::GetEncoding(866)
[Console]::OutputEncoding = [Text.Encoding]::GetEncoding(866)
$OutputEncoding = [Text.Encoding]::GetEncoding(866)
& $Entry -Event $Phase
'@
    $payload = @{workspace=$workspace;cwd=$workspace;session_id='owned-utf8-session';turn_id='owned-utf8-turn';tool_use_id='first';tool_name='mcp__serena__replace_in_files';tool_input=@{relative_path='index.ts'};transcript_path=(Join-Path $root 'own-transcript.jsonl')}
    $before = Invoke-LegacyHook pre $payload
    Assert-HookEncoding (($before | ConvertTo-Json -Compress) -eq '{}') 'UTF-8 pre-edit baseline survives an initial OEM866 console'
    Write-HookEncoding (Join-Path $workspace 'index.ts') ('export const value: number = "wrong";' + $lf)
    $after = Invoke-LegacyHook post $payload
    $feedback = $after.hookSpecificOutput.additionalContext
    Assert-HookEncoding ($feedback.Contains($workspace.Replace('\','\\')) -and $feedback.Contains('2322')) 'post fallback returns the exact Unicode workspace and actual TypeScript error'
    $payload.tool_use_id = 'second'
    $null = Invoke-LegacyHook pre $payload
    Write-HookEncoding (Join-Path $workspace 'index.ts') ('export const value: number = 2;' + $lf)
    $fixed = Invoke-LegacyHook post $payload
    Assert-HookEncoding ($fixed.hookSpecificOutput.additionalContext -match '"status": "clean"') 'the corrected Unicode workspace receives an authoritative clean result'
    Write-Output "Hook encoding checks passed: $checks assertions."
    $failed = $false
} finally {
    if ($KeepProbe -or $failed) { Write-Output "Retained owned encoding fixture: $root" }
    elseif (Test-Path -LiteralPath $root) { Remove-HookEncoding $root }
}
