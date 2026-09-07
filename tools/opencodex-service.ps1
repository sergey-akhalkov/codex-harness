#requires -Version 7.4
[CmdletBinding()]
param([Parameter(Mandatory)][string]$StatePath)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Record entry before importing dependencies: Task Scheduler hides this console.
# Keep only stage, error type/HResult and source locations, never config, command
# arguments, environment values or raw exception text that could contain tokens.
if (-not [IO.Path]::IsPathFullyQualified($StatePath)) { throw 'Service descriptor must be absolute.' }
$StatePath = [IO.Path]::GetFullPath($StatePath)
if (-not $StatePath.EndsWith('\harness\subscriptions\service.json', [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected service descriptor destination.' }
$entryDirectory = Join-Path (Split-Path $StatePath) 'runs'
$current = $entryDirectory
while ($current) {
    $item = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
    if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Service entry log cannot traverse a reparse point.' }
    $parent = Split-Path $current
    if ($parent -eq $current) { break }
    $current = $parent
}
if (-not (Test-Path -LiteralPath (Split-Path $StatePath) -PathType Container)) { throw 'Service descriptor parent is absent.' }
[void][IO.Directory]::CreateDirectory($entryDirectory)
$entryPath = Join-Path $entryDirectory ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmss') + '-' + [guid]::NewGuid().ToString('N') + '.host.jsonl')
$entryStream = [IO.File]::Open($entryPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
function Write-ServiceEntry([string]$Stage, $Failure = $null) {
    $record = @{time=[DateTime]::UtcNow.ToString('o');processId=$PID;stage=$Stage}
    if ($Failure) {
        $record.exceptionType = $Failure.Exception.GetType().FullName
        $record.hresult = $Failure.Exception.HResult
        $record.category = [string]$Failure.CategoryInfo.Category
        $record.scriptStack = $Failure.ScriptStackTrace
        $chain = @()
        $cause = $Failure.Exception
        while ($cause -and $chain.Count -lt 8) {
            $detail = @{type=$cause.GetType().FullName;hresult=$cause.HResult}
            if ($cause -is [ComponentModel.Win32Exception]) { $detail.nativeErrorCode=$cause.NativeErrorCode }
            if ($cause -is [Management.Automation.RuntimeException]) { $detail.scriptStack=$cause.ErrorRecord.ScriptStackTrace }
            $chain += $detail
            $cause = $cause.InnerException
        }
        $record.exceptionChain = $chain
    }
    $bytes = [Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json -Depth 6 -Compress) + [Environment]::NewLine)
    $entryStream.Write($bytes); $entryStream.Flush($true)
}
$stage = 'entry'
try {
    Write-ServiceEntry $stage
    $stage = 'module-import'
    Import-Module (Join-Path $PSScriptRoot 'subscription-routing.psm1')
    Write-ServiceEntry 'module-imported'
    $stage = 'service-host'
    Invoke-SubscriptionServiceHost -StatePath $StatePath
    Write-ServiceEntry 'completed'
} catch {
    Write-ServiceEntry ('failed:' + $stage) $_
    throw "Subscription service failed during $stage; private entry record: $entryPath"
} finally { $entryStream.Dispose() }
