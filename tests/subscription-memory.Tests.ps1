#requires -Version 7.4
<#
Opt-in synthetic Responses memory probe. The proxy alone has the unchanged
768 MiB job; its generator/mock and job owner live in an outer 2 GiB, 180-second
job. No global lifecycle, provider credentials, installation, or external calls.
Evidence stays in a unique temporary directory. No recursive deletion.
#>
[CmdletBinding()]
param([switch]$RunMemoryProbe, [string]$PackageRoot, [string]$ProxyRequest)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$runner = Join-Path $repository 'tools/opencodex-process.ps1'
if ($ProxyRequest) {
    $full = [IO.Path]::GetFullPath($ProxyRequest)
    $fixture = Split-Path $full -Parent
    if ((Split-Path $fixture -Leaf) -notmatch '^codex-subscription-memory-[a-f0-9]{32}$' -or (Split-Path $full -Leaf) -cnotmatch '^proxy-(cold|warm)\.request\.json$') { throw 'Invalid proxy fixture boundary.' }
    $runName = $Matches[1]
    $samplePath = Join-Path $fixture 'windows-memory.jsonl'
    $result = & $runner -RequestPath $full -ResultPath (Join-Path $fixture "proxy-$runName.result.json") -PassThru -OnRunning {
        param([uint32]$ProcessId)
        $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
        if ($process) {
            $sample = @{time=[DateTime]::UtcNow.ToString('o');pid=$ProcessId;privateBytes=$process.PrivateMemorySize64;workingSetBytes=$process.WorkingSet64;cpuSeconds=$process.TotalProcessorTime.TotalSeconds}
            [IO.File]::AppendAllText($samplePath, ($sample | ConvertTo-Json -Compress) + "`n")
        }
    }
    exit $result.ExitCode
}
if (-not $RunMemoryProbe) { 'SKIP: pass -RunMemoryProbe for the credential-free synthetic Responses memory probe.'; return }
if (-not $IsWindows) { throw 'The memory probe requires Windows Job Objects.' }
if (-not $PackageRoot) {
    $descriptor = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex/harness/subscriptions/service.json'
    $PackageRoot = (Get-Content -LiteralPath $descriptor -Raw | ConvertFrom-Json).dependency.root
}
$PackageRoot = [IO.Path]::GetFullPath($PackageRoot)
$metadata = Get-Content -LiteralPath (Join-Path $PackageRoot 'package.json') -Raw | ConvertFrom-Json
if ($metadata.name -cne '@bitkyc08/opencodex' -or $metadata.version -cne '2.44.0') { throw 'The probe requires the already installed pinned OpenCodex 2.44.0 package.' }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-memory-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($fixture)
$realUser = [Environment]::GetFolderPath('UserProfile')
$realCodex = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $realUser '.codex' }
$protected = @((Join-Path $realCodex 'config.toml'), (Join-Path $realCodex 'auth.json'), (Join-Path $realCodex 'harness.config.toml'),
    (Join-Path $realUser '.opencodex/config.json'), (Join-Path $realUser '.opencodex/auth.json'),
    (Join-Path $realUser '.config/opencode/opencode.json'), (Join-Path $realUser '.config/opencode/opencode.jsonc'), (Join-Path $realUser '.local/share/opencode/auth.json'))
$baseline = @{}
foreach ($path in $protected) { $baseline[$path] = if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path).Hash } else { $null } }
$parameters = @{fixture=$fixture;package=$PackageRoot;repository=$repository;powershell=(Get-Process -Id $PID).Path;realUser=$realUser}
$parameterPath = Join-Path $fixture 'parameters.json'
[IO.File]::WriteAllText($parameterPath, ($parameters | ConvertTo-Json))
$request = @{executable=(Join-Path $PackageRoot 'node_modules/bun/bin/bun.exe');arguments=@('--no-env-file',(Join-Path $PSScriptRoot 'subscription-memory.Tests.mjs'),$parameterPath);
    workingDirectory=$fixture;stdoutPath=(Join-Path $fixture 'controller.stdout');stderrPath=(Join-Path $fixture 'controller.stderr');environment=@{};memoryLimitMiB=2048;timeoutSeconds=180}
$requestPath = Join-Path $fixture 'controller.request.json'
[IO.File]::WriteAllText($requestPath, ($request | ConvertTo-Json -Depth 8))
$summary = @{status='running';fixture=$fixture;startedAt=[DateTime]::UtcNow.ToString('o');globalPreservation=@{};proxyLimitMiB=768;outerLimitMiB=2048;timeoutSeconds=180}
Write-Output "Synthetic memory evidence: $fixture"
try {
    $summary.controller = & $runner -RequestPath $requestPath -ResultPath (Join-Path $fixture 'controller.result.json') -PassThru
    $summary.status = if ($summary.controller.ExitCode -eq 0) { 'passed' } else { 'failed' }
} catch { $summary.status='failed'; $summary.failure=$_.Exception.Message }
finally {
    foreach ($path in $baseline.Keys) {
        $hash = if (Test-Path -LiteralPath $path -PathType Leaf) { (Get-FileHash -LiteralPath $path).Hash } else { $null }
        $summary.globalPreservation[$path] = $hash -ceq $baseline[$path]
        if ($hash -cne $baseline[$path]) { $summary.status='failed' }
    }
    $summary.completedAt=[DateTime]::UtcNow.ToString('o')
    [IO.File]::WriteAllText((Join-Path $fixture 'supervisor.json'), ($summary | ConvertTo-Json -Depth 12))
    Write-Output "Synthetic memory result: $($summary.status); report: $(Join-Path $fixture 'report.json')"
}
if ($summary.status -ne 'passed') { throw "Synthetic memory probe did not pass; inspect retained evidence in $fixture" }
