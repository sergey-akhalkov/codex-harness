#requires -Version 7.4
# Actual temporary Windows file handles; no services, credentials or model calls.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$module = Import-Module (Join-Path (Split-Path $PSScriptRoot) 'tools/code-tools.psm1') -Force -PassThru
$root = Join-Path ([IO.Path]::GetTempPath()) ('codex-code-tools-io-' + [guid]::NewGuid().ToString('N'))
$path = Join-Path $root 'pending.json'
Write-CodeToolsJson $path @{generation='before'}
try {
    # Pause the real reader at parsing, while its FileStream is still open.
    # Replace the path through the real writer, then parse the reader's snapshot.
    & $module {
        param($path)
        $script:replacementPath = $path
        function script:ConvertFrom-Json {
            param([Parameter(ValueFromPipeline)][string]$InputObject, [switch]$AsHashtable)
            process {
                Write-CodeToolsJson $script:replacementPath @{generation='after'}
                Microsoft.PowerShell.Utility\ConvertFrom-Json -InputObject $InputObject -AsHashtable:$AsHashtable
            }
        }
    } $path
    try { $snapshot = Read-CodeToolsJson $path }
    finally { & $module { Remove-Item -LiteralPath Function:ConvertFrom-Json } }
    if ($snapshot.generation -ne 'before' -or (Read-CodeToolsJson $path).generation -ne 'after') { throw 'Atomic replacement lost the old reader or new writer snapshot.' }
    'PASS actual journal reader permits atomic replacement and retains its complete prior snapshot'

    $lock = [IO.File]::Open($path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
    $failed = $false
    try { try { Write-CodeToolsJson $path @{generation='must-not-commit'} } catch { $failed = $true } }
    finally { $lock.Dispose() }
    if (-not $failed -or (Read-CodeToolsJson $path).generation -ne 'after') { throw 'A foreign non-sharing reader was bypassed or the prior journal was damaged.' }
    if (@(Get-ChildItem -LiteralPath $root -Filter '*.tmp').Count) { throw 'Failed writer left temporary files.' }
    'PASS foreign file lock fails visibly, preserves prior bytes and cleans its temporary file'

    [IO.File]::WriteAllText($path, '{"generation":"BOM"}', [Text.UTF8Encoding]::new($true))
    if ((Read-CodeToolsJson $path).generation -ne 'BOM') { throw 'UTF-8 BOM compatibility was lost.' }
    Remove-CodeToolsFile $path
    if ($null -ne (Read-CodeToolsJson $path)) { throw 'Absent journal did not return null.' }
    'PASS UTF-8 BOM and absent journal compatibility'
} finally {
    # Only this known file and its now-empty test directory are removed.
    if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path }
    if (-not @(Get-ChildItem -LiteralPath $root -Force).Count) { Remove-Item -LiteralPath $root }
}
