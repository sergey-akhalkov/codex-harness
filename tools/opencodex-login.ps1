#requires -Version 7.4
# Host-side browser login. Bun receives neither interactive stdin nor the browser process.
[CmdletBinding()]
param(
    [string]$PackageRoot,
    [string]$OpencodexHome = (Join-Path $env:USERPROFILE '.opencodex'),
    [string]$CodexHome = $(if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }),
    [string]$EvidenceRoot,
    [switch]$NoOpenBrowser
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This login entry requires Windows.' }
if (-not $PackageRoot) {
    $servicePath = Join-Path $CodexHome 'harness/subscriptions/service.json'
    if (Test-Path -LiteralPath $servicePath -PathType Leaf) {
        $descriptor = Get-Content -LiteralPath $servicePath -Raw | ConvertFrom-Json
        if ($descriptor.owner -ne 'codex-harness-subscriptions') { throw 'Unexpected subscription service owner.' }
        $PackageRoot = $descriptor.dependency.root
    } else {
        $PackageRoot = Join-Path (Split-Path (Get-Command npm -ErrorAction Stop).Source) 'node_modules/@bitkyc08/opencodex'
    }
}
foreach ($path in @($PackageRoot,$OpencodexHome,$CodexHome)) {
    if (-not [IO.Path]::IsPathFullyQualified($path)) { throw 'Package and runtime homes must be absolute paths.' }
}
if (-not (Test-Path -LiteralPath (Join-Path $OpencodexHome 'config.json') -PathType Leaf)) { throw 'Prepare the OpenCodex configuration before login.' }
if (-not $EvidenceRoot) { $EvidenceRoot = Join-Path $CodexHome 'harness/runtime/subscription-login' }
$runDirectory = Join-Path $EvidenceRoot ([guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($runDirectory)
$authorizationPath = Join-Path $runDirectory 'authorization.json'
$requestPath = Join-Path $runDirectory 'request.json'
$resultPath = Join-Path $runDirectory 'result.json'
$request = @{
    executable = Join-Path $PackageRoot 'node_modules/bun/bin/bun.exe'
    arguments = @('--no-env-file',(Join-Path $PSScriptRoot 'opencodex-login.mjs'),$PackageRoot,'xai',$authorizationPath)
    workingDirectory = $runDirectory
    stdoutPath = Join-Path $runDirectory 'stdout.log'
    stderrPath = Join-Path $runDirectory 'stderr.log'
    environment = @{ CODEX_HOME=$CodexHome; OPENCODEX_HOME=$OpencodexHome }
    memoryLimitMiB = 768
    timeoutSeconds = 360
}
$request | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $requestPath -Encoding utf8NoBOM
$start = [Diagnostics.ProcessStartInfo]::new((Join-Path $PSHOME 'pwsh.exe'))
$start.UseShellExecute = $false
$start.CreateNoWindow = $true
foreach ($argument in @('-NoLogo','-NoProfile','-File',(Join-Path $PSScriptRoot 'opencodex-process.ps1'),'-RequestPath',$requestPath,'-ResultPath',$resultPath)) { $start.ArgumentList.Add($argument) }
$child = [Diagnostics.Process]::Start($start)
$opened = $false
Write-Output "Login evidence: $runDirectory"
try {
    while (-not $child.HasExited) {
        $pagePath = $authorizationPath + '.html'
        if (-not $opened -and (Test-Path -LiteralPath $pagePath)) {
            if ($NoOpenBrowser) { Write-Output "Open this local login page: $pagePath" }
            else {
                $browser = [Diagnostics.ProcessStartInfo]::new($pagePath)
                $browser.UseShellExecute = $true
                [void][Diagnostics.Process]::Start($browser)
                Write-Output 'Local login page opened. Follow its xAI link, then paste a one-time code into the local form if xAI provides one.'
            }
            $opened = $true
        }
        Start-Sleep -Milliseconds 250
    }
    if (-not (Test-Path -LiteralPath $resultPath)) { throw "Login supervisor failed before writing a result. Evidence: $runDirectory" }
    $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
    $events = @(Get-Content -LiteralPath $request.stdoutPath)
    if ($result.ExitCode -ne 0 -or 'login_saved' -notin $events) { throw "Grok login was not confirmed (process status: $($result.Status), exit: $($result.ExitCode)). Evidence: $runDirectory" }
    [pscustomobject]@{ Authenticated=$true; Provider='xai'; PeakJobMemoryBytes=$result.PeakJobMemoryBytes; Evidence=$runDirectory }
} finally {
    if (-not $child.HasExited) { $child.Kill(); [void]$child.WaitForExit(10000) }
    $child.Dispose()
    foreach ($ownedFile in @($authorizationPath,($authorizationPath + '.html'))) {
        if (Test-Path -LiteralPath $ownedFile -PathType Leaf) { Remove-Item -LiteralPath $ownedFile -Force }
    }
}
