#requires -Version 7.4
# Read-only pinned dependency validation, isolated source/home fixtures, no network
# requests or services. Each actual Bun validator uses a 768 MiB job and 30s cap.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$PackageRoot)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
$root = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-config-' + [guid]::NewGuid().ToString('N'))
$source = Join-Path $root 'source'
$roles = Join-Path $source 'global/opencodex/agents'
[void][IO.Directory]::CreateDirectory($roles)
[void][IO.Directory]::CreateDirectory((Join-Path $root 'codex'))
[void][IO.Directory]::CreateDirectory((Join-Path $root 'opencodex'))
$configPath = Join-Path $source 'global/opencodex/config.json'
$rolePath = Join-Path $roles 'middle.toml'
$cleanConfig = [IO.File]::ReadAllText((Join-Path $repo 'global/opencodex/config.json'))
$cleanRole = [IO.File]::ReadAllText((Join-Path $repo 'global/opencodex/agents/middle.toml'))
foreach ($scenario in @('valid','api-keys','role-token','implicit-search','implicit-vision','wrong-middle','recursive-middle','unverified-grok-wire','no-empty-recovery','no-terminal-recovery')) {
    [IO.File]::WriteAllText($configPath, $cleanConfig)
    [IO.File]::WriteAllText($rolePath, $cleanRole)
    if ($scenario -eq 'api-keys') {
        $config = $cleanConfig | ConvertFrom-Json -AsHashtable
        $config.apiKeys = @(@{key='synthetic-secret-must-not-print';label='fixture'})
        [IO.File]::WriteAllText($configPath, ($config | ConvertTo-Json -Depth 30))
    }
    if ($scenario -eq 'role-token') { [IO.File]::AppendAllText($rolePath, "`nauth_token = 'synthetic-secret-must-not-print'`n") }
    if ($scenario -in @('implicit-search','implicit-vision')) {
        $candidate = $cleanConfig | ConvertFrom-Json -AsHashtable
        if ($scenario -eq 'implicit-search') { $candidate.Remove('webSearchSidecar') }
        else { $candidate.Remove('visionSidecar') }
        [IO.File]::WriteAllText($configPath, ($candidate | ConvertTo-Json -Depth 30))
    }
    if ($scenario -eq 'wrong-middle') { [IO.File]::WriteAllText($rolePath, $cleanRole.Replace('grok-4.6','grok-4.5')) }
    if ($scenario -eq 'unverified-grok-wire') {
        $candidate = $cleanConfig | ConvertFrom-Json -AsHashtable
        [void]$candidate.providers.xai.Remove('modelAdapters')
        [IO.File]::WriteAllText($configPath, ($candidate | ConvertTo-Json -Depth 30))
    }
    if ($scenario -eq 'recursive-middle') { [IO.File]::WriteAllText($rolePath, $cleanRole.Replace('enabled = false','enabled = true')) }
    if ($scenario -in @('no-empty-recovery','no-terminal-recovery')) {
        $candidate = $cleanConfig | ConvertFrom-Json -AsHashtable
        if ($scenario -eq 'no-empty-recovery') { $candidate.emptyCompletionRetry = $false }
        else { $candidate.providers.xai.terminalContinuationGuard = $false }
        [IO.File]::WriteAllText($configPath, ($candidate | ConvertTo-Json -Depth 30))
    }
    $prefix = Join-Path $root $scenario
    $request = @{executable=Join-Path $PackageRoot 'node_modules/bun/bin/bun.exe'
        arguments=@('--no-env-file',(Join-Path $repo 'tools/opencodex-config-check.mjs'),$PackageRoot,$source)
        workingDirectory=$root;environment=@{CODEX_HOME=(Join-Path $root 'codex');OPENCODEX_HOME=(Join-Path $root 'opencodex')}
        stdoutPath=$prefix+'.stdout';stderrPath=$prefix+'.stderr';memoryLimitMiB=768;timeoutSeconds=30}
    [IO.File]::WriteAllText(($prefix+'.request.json'), ($request | ConvertTo-Json -Depth 8))
    $result = & (Join-Path $repo 'tools/opencodex-process.ps1') -RequestPath ($prefix+'.request.json') -ResultPath ($prefix+'.result.json') -PassThru
    $expected = if ($scenario -eq 'valid') { 0 } else { 1 }
    if ($result.ExitCode -ne $expected -or -not $result.AssignedBeforeResume) { throw "Unexpected bounded validation result: $scenario" }
    $output = [IO.File]::ReadAllText($request.stdoutPath) + [IO.File]::ReadAllText($request.stderrPath)
    if ($output.Contains('synthetic-secret-must-not-print')) { throw 'Validator exposed a credential fixture.' }
    "PASS actual contained source validator: $scenario"
}
"Private config test evidence: $root"
