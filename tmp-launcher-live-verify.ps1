[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RepoRoot,
  [Parameter(Mandatory = $true)][string]$LiveCodexHome,
  [int]$Port = 56122
)
$ErrorActionPreference = "Stop"

function Test-ShimListening {
    $connections = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    return $connections.Count -gt 0
}

$root = Join-Path ([IO.Path]::GetTempPath()) ("xai-launcher-verify-" + [guid]::NewGuid().ToString("N"))
$codexHomeDir = Join-Path $root 'codex-home'
$bridge = $null
$launcherPath = $null
try {
    $user = Join-Path $root 'user-home'
    $workspace = Join-Path $root 'workspace'
    foreach ($d in @($codexHomeDir, $user, $workspace, "$codexHomeDir/harness/subscriptions")) { [void][IO.Directory]::CreateDirectory($d) }
    Copy-Item (Join-Path $LiveCodexHome 'harness/installation.json') (Join-Path $codexHomeDir 'harness/installation.json')
    Copy-Item (Join-Path $LiveCodexHome 'harness/subscriptions/xai-oauth.json') (Join-Path $codexHomeDir 'harness/subscriptions/xai-oauth.json')
    $registration = Get-Content -Raw (Join-Path $LiveCodexHome 'harness/installation.json') | ConvertFrom-Json
    $bridge = $registration.configBridge
    $launcherPath = $registration.launcherSource
    if (-not (Test-Path $launcherPath)) { throw 'managed launcher missing after CoreOnly install' }
    & $bridge install --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home $user | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'isolated subscriptions install failed' }
    $profile = Get-Content -Raw (Join-Path $codexHomeDir 'xai.config.toml')
    if ($profile -notmatch [regex]::Escape("http://127.0.0.1:$Port/v1")) { throw 'installed profile does not point at the shim port' }

    if (Test-ShimListening) { throw "port $Port already has a listener before the negative checks" }
    $env:CODEX_HOME = $codexHomeDir
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath -V 1> (Join-Path $root 'neg-version-out.txt') 2> (Join-Path $root 'neg-version-err.txt')
    $negVersionExit = $LASTEXITCODE
    & pwsh -NoProfile -File $launcherPath --profile zai -V 1> (Join-Path $root 'neg-zai-out.txt') 2> (Join-Path $root 'neg-zai-err.txt')
    $negZaiExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "negative version exit: $negVersionExit; negative zai exit: $negZaiExit"
    if (Test-ShimListening) { throw 'ordinary/version or zai invocation started the shim' } else { Write-Output 'PASS: shim not started by non-xai invocations' }

    $marker = "LAUNCH-" + [guid]::NewGuid().ToString('N').Substring(0, 10)
    $prompt = "Use one shell command to write exactly $marker into proof.txt in the current directory. Then read the file and print its content in your final message. No other work."
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec --profile xai --skip-git-repo-check -C $workspace --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' $prompt 1> (Join-Path $root 'out.txt') 2> (Join-Path $root 'err.txt')
    $exit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "positive xai exit: $exit"
    Get-Content (Join-Path $root 'err.txt') -Tail 6
    Get-Content (Join-Path $root 'out.txt') -Tail 3
    if (-not (Test-ShimListening)) { throw 'xai invocation did not leave the shim listening' } else { Write-Output 'PASS: shim started by xai invocation' }
    $proof = Join-Path $workspace 'proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -eq $marker)) { Write-Output 'PASS: tool call completed end-to-end through the launcher-started shim' } else { Write-Output 'FAIL: proof missing' }
    if ((Get-Content -Raw (Join-Path $root 'out.txt')) -match [regex]::Escape($marker)) { Write-Output 'PASS: final answer carries marker' } else { Write-Output 'FAIL: marker missing' }
    if ($exit -ne 0) { throw "xai launcher invocation exited $exit" }
} finally {
    Remove-Item env:CODEX_HOME -ErrorAction SilentlyContinue
    $listeners = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        if ($listener.OwningProcess) { Stop-Process -Id $listener.OwningProcess -Force -ErrorAction SilentlyContinue }
    }
    if ($bridge -and (Test-Path (Join-Path $codexHomeDir 'xai.config.toml'))) {
        $ErrorActionPreference = 'Continue'
        & $bridge disconnect --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home (Join-Path $root 'user-home') | Out-Null
    }
    Write-Output ("Private evidence: " + $root)
}
