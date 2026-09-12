#requires -Version 7.4
# Bounded live check of the preserved local zai profile. Does not stop OpenCodex.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
$pwsh = 'C:\Program Files\PowerShell\7\pwsh.exe'
if (-not (Test-Path -LiteralPath $pwsh)) { throw 'Desktop PowerShell 7 is required.' }
$launcher = Join-Path $env:USERPROFILE '.codex\harness\bin\codex.ps1'
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('codex-zai-profile-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($evidence)
$marker = 'ZAI_PROFILE_OK_' + [guid]::NewGuid().ToString('N').Substring(0,8)
[IO.File]::WriteAllText((Join-Path $evidence 'marker.txt'), $marker)
$prompt = "Read marker.txt in the current directory with one local shell command and return its exact contents. Do not use network, MCP, or write operations."
$request = @{
    executable = $pwsh
    arguments = @('-NoLogo','-NoProfile','-File',$launcher,'--profile','zai','exec','--skip-git-repo-check','--json',$prompt)
    workingDirectory = $evidence
    stdoutPath = Join-Path $evidence 'stdout.jsonl'
    stderrPath = Join-Path $evidence 'stderr.txt'
    startedPath = Join-Path $evidence 'started.json'
    environment = @{ CODEX_HOME = (Join-Path $env:USERPROFILE '.codex') }
    memoryLimitMiB = 2048
    timeoutSeconds = 90
}
[IO.File]::WriteAllText((Join-Path $evidence 'request.json'), ($request | ConvertTo-Json -Depth 8))
$result = & (Join-Path $repo 'tools\opencodex-process.ps1') -RequestPath (Join-Path $evidence 'request.json') -ResultPath (Join-Path $evidence 'result.json') -PassThru
$stdout = if (Test-Path (Join-Path $evidence 'stdout.jsonl')) { Get-Content -LiteralPath (Join-Path $evidence 'stdout.jsonl') -Raw } else { '' }
$stderr = if (Test-Path (Join-Path $evidence 'stderr.txt')) { Get-Content -LiteralPath (Join-Path $evidence 'stderr.txt') -Raw } else { '' }
$combined = $stdout + $stderr
$hasGlm = $combined.Contains('glm-5.3')
$hasProxy = $combined.Contains('127.0.0.1:10100')
$hasChat = $combined.Contains('coding/paas')
$hasMarker = $combined.Contains($marker)
$profileHash = (Get-FileHash -LiteralPath (Join-Path $env:USERPROFILE '.codex\zai.config.toml')).Hash
$catalogHash = (Get-FileHash -LiteralPath (Join-Path $env:USERPROFILE '.codex\zai.models.json')).Hash
[pscustomobject]@{
    Evidence = $evidence
    Status = $result.Status
    ExitCode = $result.ExitCode
    HasGlm = $hasGlm
    HasProxy = $hasProxy
    HasChat = $hasChat
    HasMarker = $hasMarker
    ProfileHash = $profileHash
    CatalogHash = $catalogHash
    StdoutLen = $stdout.Length
    StderrLen = $stderr.Length
} | ConvertTo-Json -Compress
