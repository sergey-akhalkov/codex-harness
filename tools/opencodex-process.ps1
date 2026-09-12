#requires -Version 7.4
# A request file keeps environment values and argument serialization out of the
# launcher command line. Never place credentials in the child arguments array.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$RequestPath,
    [string]$ResultPath,
    [switch]$PassThru,
    # Trusted caller code only; never loaded from the JSON request. Called on
    # this thread while the Windows job remains owned by this process.
    [scriptblock]$OnRunning
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-BoundedPlainPath([string]$Value, [string]$Kind, [switch]$MayBeAbsent) {
    if (-not [IO.Path]::IsPathFullyQualified($Value)) { throw "$Kind must be an absolute path." }
    $full = [IO.Path]::GetFullPath($Value)
    $current = $full
    while ($current) {
        $entry = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
        if ($entry -and ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "$Kind must not traverse a reparse point." }
        $parent = Split-Path $current -Parent
        if ($parent -eq $current) { break }
        $current = $parent
    }
    if (-not $MayBeAbsent -and -not (Test-Path -LiteralPath $full)) { throw "$Kind does not exist." }
    $full
}

if (-not $IsWindows) { throw 'The bounded OpenCodex runner requires Windows.' }
$RequestPath = Get-BoundedPlainPath $RequestPath 'Request file'
$request = Get-Content -LiteralPath $RequestPath -Raw | ConvertFrom-Json -AsHashtable
$allowed = @('executable','arguments','workingDirectory','stdoutPath','stderrPath','environment','secretFiles','memoryLimitMiB','timeoutSeconds','startedPath')
foreach ($key in $request.Keys) { if ($key -cnotin $allowed) { throw "Unknown bounded process request field: $key" } }
foreach ($key in @('executable','workingDirectory','stdoutPath','stderrPath')) {
    if (-not $request.ContainsKey($key) -or $request[$key] -isnot [string]) { throw "Bounded process request needs string field $key." }
}
$executable = Get-BoundedPlainPath $request.executable 'Executable'
if ([IO.Path]::GetExtension($executable) -ine '.exe' -or -not (Test-Path -LiteralPath $executable -PathType Leaf)) { throw 'Executable must be an explicit .exe file.' }
$directory = Get-BoundedPlainPath $request.workingDirectory 'Working directory'
if (-not (Test-Path -LiteralPath $directory -PathType Container)) { throw 'Working directory must be a directory.' }
$stdout = Get-BoundedPlainPath $request.stdoutPath 'Stdout file' -MayBeAbsent
$stderr = Get-BoundedPlainPath $request.stderrPath 'Stderr file' -MayBeAbsent
if ($stdout -ieq $stderr -or (Test-Path -LiteralPath $stdout) -or (Test-Path -LiteralPath $stderr)) { throw 'Stdout and stderr need distinct new file paths for each launch.' }
foreach ($outputPath in @($stdout,$stderr)) {
    if (-not (Test-Path -LiteralPath (Split-Path $outputPath) -PathType Container)) { throw 'Create the private output directory before launching a bounded process.' }
}
if ($ResultPath) {
    $ResultPath = Get-BoundedPlainPath $ResultPath 'Result file' -MayBeAbsent
    if ($ResultPath -ieq $stdout -or $ResultPath -ieq $stderr -or (Test-Path -LiteralPath $ResultPath)) { throw 'Result needs a separate new file path.' }
    if (-not (Test-Path -LiteralPath (Split-Path $ResultPath) -PathType Container)) { throw 'Result parent directory does not exist.' }
}
$startedPath = $null
if ($request.ContainsKey('startedPath')) {
    if ($request.startedPath -isnot [string]) { throw 'startedPath must be a string.' }
    $startedPath = Get-BoundedPlainPath $request.startedPath 'Started receipt' -MayBeAbsent
    if ($startedPath -in @($stdout,$stderr,$ResultPath) -or (Test-Path -LiteralPath $startedPath)) { throw 'Started receipt needs a separate new file path.' }
    if (-not (Test-Path -LiteralPath (Split-Path $startedPath) -PathType Container)) { throw 'Started receipt parent directory does not exist.' }
}
$childArguments = @()
if ($request.ContainsKey('arguments')) {
    if ($request.arguments -isnot [array]) { throw 'Arguments must be a JSON array of strings.' }
    foreach ($argument in $request.arguments) {
        if ($argument -isnot [string] -or $argument.Contains([char]0)) { throw 'Arguments must be strings without NUL.' }
        $childArguments += $argument
    }
}
$environment = if ($request.ContainsKey('environment')) { $request.environment } else { @{} }
if ($environment -isnot [Collections.IDictionary]) { throw 'Environment must be an object.' }
if ($request.ContainsKey('secretFiles')) {
    if ($request.secretFiles -isnot [Collections.IDictionary]) { throw 'secretFiles must be an object of environment names to file paths.' }
    $injected = [ordered]@{}
    foreach ($name in @($request.secretFiles.Keys)) {
        if ($name -cnotmatch '^[A-Z][A-Z0-9_]*$') { throw 'Secret environment names must be uppercase identifiers.' }
        if ($environment.Contains($name)) { throw "Request environment must not include secret name $name." }
        $secretPath = $request.secretFiles[$name]
        if ($secretPath -isnot [string]) { throw "Secret file for $name must be a string path." }
        $secretPath = Get-BoundedPlainPath $secretPath 'Secret file'
        if (-not (Test-Path -LiteralPath $secretPath -PathType Leaf)) { throw "Secret file for $name does not exist." }
        $info = Get-Item -LiteralPath $secretPath -Force
        if ($info.Length -le 0 -or $info.Length -gt 8192) { throw "Secret file for $name must be between 1 and 8192 bytes." }
        $bytes = [IO.File]::ReadAllBytes($secretPath)
        if ($bytes.IndexOf([byte]0) -ge 0) { throw "Secret file for $name must not contain NUL." }
        $value = [Text.UTF8Encoding]::new($false).GetString($bytes).Trim()
        if (-not $value -or $value -match '[\r\n]') { throw "Secret file for $name must be a single non-empty line." }
        $injected[$name] = $value
    }
    foreach ($name in $injected.Keys) { $environment[$name] = $injected[$name] }
}
$memoryMiB = if ($request.ContainsKey('memoryLimitMiB')) { $request.memoryLimitMiB } else { 768 }
$timeoutSeconds = if ($request.ContainsKey('timeoutSeconds')) { $request.timeoutSeconds } else { 300 }
if ($memoryMiB -isnot [long] -and $memoryMiB -isnot [int]) { throw 'memoryLimitMiB must be an integer.' }
if ($memoryMiB -lt 32 -or $memoryMiB -gt 32768) { throw 'memoryLimitMiB must be between 32 and 32768.' }
if (($timeoutSeconds -isnot [long] -and $timeoutSeconds -isnot [int]) -or $timeoutSeconds -lt 0 -or $timeoutSeconds -gt 604800) { throw 'timeoutSeconds must be an integer from 0 through 604800; 0 is a persistent foreground process.' }

if (-not ('CodexHarness.SubscriptionProcess.Runner' -as [type])) {
    Add-Type -LiteralPath (Join-Path $PSScriptRoot 'opencodex-process.cs')
}
$onAssigned = if ($startedPath) { [Action[uint32]] {
    param([uint32]$ProcessId)
    $receiptBytes = [Text.Encoding]::UTF8.GetBytes((@{ processId = $ProcessId; assignedBeforeResume = $true } | ConvertTo-Json -Compress))
    $receiptStream = [IO.File]::Open($startedPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $receiptStream.Write($receiptBytes); $receiptStream.Flush($true) } finally { $receiptStream.Dispose() }
} } else { $null }
$result = [CodexHarness.SubscriptionProcess.Runner]::Run($executable, [string[]]$childArguments, $directory, $stdout, $stderr,
    ([uint64]$memoryMiB * 1MB), ([long]$timeoutSeconds * 1000), $environment, $onAssigned, $(if ($OnRunning) { [Action[uint32]]$OnRunning } else { $null }))
if ($ResultPath) {
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($result | ConvertTo-Json -Depth 8))
    $stream = [IO.File]::Open($ResultPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
    try { $stream.Write($bytes); $stream.Flush($true) } finally { $stream.Dispose() }
}
if ($PassThru) { return $result }
$result | ConvertTo-Json -Compress
exit $result.ExitCode
