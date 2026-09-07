#requires -Version 7.4
# A hung native substitute tests deadline/ownership, not native config semantics.
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$root=Join-Path $env:TEMP ('harness-diagnostic-failure-'+[guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $root
$priorPidFile=$env:HARNESS_DIAGNOSTIC_TEST_PID
try {
    $source=@'
using System;
using System.IO;
using System.Threading;
class HungConsumer {
  static void Main() {
    File.WriteAllText(Environment.GetEnvironmentVariable("HARNESS_DIAGNOSTIC_TEST_PID"), System.Diagnostics.Process.GetCurrentProcess().Id.ToString());
    Console.Error.WriteLine("PRIVATE_NATIVE_ERROR_SENTINEL");
    Thread.Sleep(60000);
  }
}
'@
    $sourceFile=Join-Path $root 'hung.cs'
    $executable=Join-Path $root 'hung.exe'
    [IO.File]::WriteAllText($sourceFile,$source)
    $compiler=Join-Path $env:WINDIR 'Microsoft.NET/Framework64/v4.0.30319/csc.exe'
    & $compiler /nologo ('/out:'+$executable) $sourceFile
    if ($LASTEXITCODE) { throw 'Could not build timeout fixture' }
    $env:HARNESS_DIAGNOSTIC_TEST_PID="$root/child.pid"
    $parentPid=$PID
    Import-Module (Join-Path $repo 'tools/source-diagnostics.psm1') -Force
    $timer=[Diagnostics.Stopwatch]::StartNew()
    $report=Invoke-HarnessSourceDiagnostics -SourceRoot $repo -UserHome $root -CodexHome $root -ProjectPath $root -CodexCommand "$root/hung.exe" -TimeoutSeconds 1
    if ($report.status -ne 'incomplete' -or -not @($report.findings | Where-Object code -eq 'native-timeout').Count) { throw 'Timeout not classified' }
    if ($timer.Elapsed.TotalSeconds -gt 10) { throw 'Deadline plus cleanup exceeded bound' }
    if (($report | ConvertTo-Json -Depth 25).Contains('PRIVATE_NATIVE_ERROR_SENTINEL')) { throw 'Native error leaked' }
    $childPid=[int](Get-Content -LiteralPath "$root/child.pid")
    if (Get-Process -Id $childPid -ErrorAction SilentlyContinue) { throw 'Owned timeout process survived' }
    if (-not (Get-Process -Id $parentPid -ErrorAction SilentlyContinue)) { throw 'Parent was affected' }
    Write-Output 'PASS: finite deadline, owned-child cleanup, parent preservation, stderr suppression, incomplete status.'
} finally {
    $env:HARNESS_DIAGNOSTIC_TEST_PID=$priorPidFile
    $full=[IO.Path]::GetFullPath($root)
    if (-not $full.StartsWith([IO.Path]::GetFullPath($env:TEMP).TrimEnd('\')+'\harness-diagnostic-failure-',[StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid cleanup root' }
    # This fixture contains only ordinary files produced above, no links.
    Remove-Item -LiteralPath $full -Recurse -Force
}
