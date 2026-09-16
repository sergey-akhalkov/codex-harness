[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$RepoRoot)
$ErrorActionPreference = 'Stop'
$liveHome = 'C:\Users\noilw\.codex'
$registration = Get-Content -Raw (Join-Path $liveHome 'harness/installation.json') | ConvertFrom-Json
$launcherPath = $registration.launcherSource

Write-Output '=== real-home wiring ==='
$profile = Get-Content -Raw (Join-Path $liveHome 'xai.config.toml')
if ($profile -match [regex]::Escape('http://127.0.0.1:56122/v1')) { Write-Output 'PASS: xai profile points at the shim' } else { Write-Output 'FAIL: xai profile base_url' }
if ($profile -match 'xai-token') { Write-Output 'PASS: xai profile uses the token helper' } else { Write-Output 'FAIL: token helper missing' }
$ordinary = Get-Content -Raw (Join-Path $liveHome 'config.toml')
if ($ordinary -match 'gpt-6-astra' -and $ordinary -notmatch 'openai_base_url' -and $ordinary -notmatch '127\.0\.0\.1:10100') { Write-Output 'PASS: ordinary config is native Astra without proxy injection' } else { Write-Output 'FAIL: ordinary config injection present' }
Write-Output ('task 10100 listeners: ' + @(Get-NetTCPConnection -LocalPort 10100 -State Listen -ErrorAction SilentlyContinue).Count)
Write-Output ('shim listeners before acceptance: ' + @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue).Count)

$repo = Join-Path ([IO.Path]::GetTempPath()) ('xai-acceptance-repo-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
git init $repo | Out-Null
try {
    Push-Location $repo
    git config user.email 'acceptance@example.invalid'
    git config user.name 'acceptance'
    'readme' | Set-Content README.md
    git add README.md
    git commit -m init | Out-Null
    Pop-Location

    $marker = "REAL-" + [guid]::NewGuid().ToString('N').Substring(0, 10)
    $out = Join-Path $repo 'out.txt'
    $err = Join-Path $repo 'err.txt'
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec --profile xai -C $repo --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' "Use one shell command to write exactly $marker into proof.txt in the current directory. Then read the file and print its content in your final message. No other work." 1> $out 2> $err
    $xaiExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "=== xai from another repository: exit $xaiExit ==="
    $header = (Get-Content -Raw $err) -split "`n" | Select-String -Pattern 'model:|provider:'
    Write-Output ($header -join ' | ')
    Get-Content $err -Tail 4
    Get-Content $out -Tail 3
    $proof = Join-Path $repo 'proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -eq $marker)) { Write-Output 'PASS: real-home tool turn completed through shim' } else { Write-Output 'FAIL: proof missing' }
    if ((Get-Content -Raw $out) -match [regex]::Escape($marker)) { Write-Output 'PASS: marker returned' } else { Write-Output 'FAIL: marker missing' }
    Write-Output ('shim listeners after xai turn: ' + @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue).Count)

    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec -C $repo --dangerously-bypass-approvals-and-sandbox 'Reply with the single word PONG and nothing else.' 1> (Join-Path $repo 'ordinary-out.txt') 2> (Join-Path $repo 'ordinary-err.txt')
    $ordinaryExit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "=== ordinary session: exit $ordinaryExit ==="
    $ordinaryHeader = (Get-Content -Raw (Join-Path $repo 'ordinary-err.txt')) -split "`n" | Select-String -Pattern 'model:|provider:'
    Write-Output ($ordinaryHeader -join ' | ')
    Write-Output ((Get-Content -Raw (Join-Path $repo 'ordinary-out.txt')) -replace "`r?`n", ' ')
    if ($xaiExit -ne 0 -or $ordinaryExit -ne 0) { throw "acceptance exits: xai=$xaiExit ordinary=$ordinaryExit" }
} finally {
    Push-Location $RepoRoot
    Pop-Location
    $listeners = @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        if ($listener.OwningProcess) { Stop-Process -Id $listener.OwningProcess -Force -ErrorAction SilentlyContinue }
    }
    Write-Output ("Private acceptance repo: " + $repo)
}
