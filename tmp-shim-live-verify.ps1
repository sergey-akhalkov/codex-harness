[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string]$RepoRoot,
  [Parameter(Mandatory = $true)][string]$LiveCodexHome,
  [int]$Port = 57661
)
$ErrorActionPreference = "Stop"

$root = Join-Path ([IO.Path]::GetTempPath()) ("xai-shim-verify-" + [guid]::NewGuid().ToString("N"))
$shimExe = Join-Path $RepoRoot 'target\debug\codex-harness.exe'
if (-not (Test-Path $shimExe)) { throw 'debug codex-harness.exe not built' }
$shim = Start-Process -FilePath $shimExe -ArgumentList @('xai-responses-shim', '--port', "$Port") -PassThru -WindowStyle Hidden
$ready = $false
foreach ($i in 1..40) {
    if ($shim.HasExited) { break }
    try { $c = New-Object Net.Sockets.TcpClient; $c.Connect('127.0.0.1', $Port); $ready = $true; $c.Close(); break } catch { Start-Sleep -Milliseconds 250 }
}
if (-not $ready) { Stop-Process -Id $shim.Id -Force -ErrorAction SilentlyContinue; throw 'shim did not start' }

$bridge = $null
$codexHomeDir = Join-Path $root 'codex-home'
try {
    $user = Join-Path $root 'user-home'
    $workspace = Join-Path $root 'workspace'
    foreach ($d in @($codexHomeDir, $user, $workspace, "$codexHomeDir/harness/subscriptions")) { [void][IO.Directory]::CreateDirectory($d) }
    Copy-Item (Join-Path $LiveCodexHome 'harness/installation.json') (Join-Path $codexHomeDir 'harness/installation.json')
    Copy-Item (Join-Path $LiveCodexHome 'harness/subscriptions/xai-oauth.json') (Join-Path $codexHomeDir 'harness/subscriptions/xai-oauth.json')
    $installation = Get-Content -Raw (Join-Path $LiveCodexHome 'harness/installation.json') | ConvertFrom-Json
    $bridge = $installation.configBridge
    & $bridge install --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home $user | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'isolated subscriptions install failed' }
    $profilePath = Join-Path $codexHomeDir 'xai.config.toml'
    $profile = Get-Content -Raw $profilePath
    $profile = $profile -replace 'https://api\.x\.ai/v1', ("http://127.0.0.1:$Port/v1")
    [IO.File]::WriteAllText($profilePath, $profile)

    $env:CODEX_HOME = $codexHomeDir
    $codexCli = $installation.codexCommand
    $marker = "SHIM-" + [guid]::NewGuid().ToString('N').Substring(0, 10)
    $prompt = "Use one shell command to write exactly $marker into proof.txt in the current directory. Then read the file and print its content in your final message. No other work."
    $ErrorActionPreference = 'Continue'
    & $codexCli exec --profile xai --skip-git-repo-check -C $workspace --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' $prompt 1> (Join-Path $root 'out.txt') 2> (Join-Path $root 'err.txt')
    $exit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "codex exit: $exit"
    Get-Content (Join-Path $root 'err.txt') -Tail 10
    Get-Content (Join-Path $root 'out.txt') -Tail 4
    $proof = Join-Path $workspace 'proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -eq $marker)) { Write-Output 'PROOF: tool call completed end-to-end' } else { Write-Output 'PROOF: MISSING' }
    if ((Get-Content -Raw (Join-Path $root 'out.txt')) -match [regex]::Escape($marker)) { Write-Output 'MARKER: final answer carries marker' } else { Write-Output 'MARKER: missing' }
} finally {
    Remove-Item env:CODEX_HOME -ErrorAction SilentlyContinue
    if ($bridge -and (Test-Path (Join-Path $codexHomeDir 'xai.config.toml'))) {
        & $bridge disconnect --subscriptions-only --source $RepoRoot --codex-home $codexHomeDir --user-home (Join-Path $root 'user-home') | Out-Null
    }
    if (-not $shim.HasExited) { Stop-Process -Id $shim.Id -Force }
    Write-Output ("Private evidence: " + $root)
}
