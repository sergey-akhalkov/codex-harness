[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$RepoRoot)
$ErrorActionPreference = 'Stop'
$registration = Get-Content -Raw 'C:\Users\noilw\.codex\harness\installation.json' | ConvertFrom-Json
$launcherPath = $registration.launcherSource
$repo = Join-Path ([IO.Path]::GetTempPath()) ('xai-parity-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
git init $repo | Out-Null
try {
    Push-Location $repo
    git config user.email 'parity@example.invalid'
    git config user.name 'parity'
    'initial' | Set-Content README.md
    git add README.md
    git commit -m init | Out-Null
    Pop-Location
    $out = Join-Path $repo 'out.txt'
    $err = Join-Path $repo 'err.txt'
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec --profile xai -C $repo --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' "Use apply_patch to create parity-proof.txt containing exactly PARITY-OK (single line, no trailing spaces). Then show the file content with one shell command." 1> $out 2> $err
    $exit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "apply_patch turn exit: $exit"
    ((Get-Content -Raw $err) -split "`n" | Select-String -Pattern 'model:|provider:|apply_patch|warning') | ForEach-Object { $_.Line }
    Get-Content $out -Tail 3
    $proof = Join-Path $repo 'parity-proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -eq 'PARITY-OK')) { Write-Output 'PASS: live apply_patch through the shim' } else { Write-Output 'FAIL: parity proof missing' }
    if ((Get-Content -Raw $out) -match 'PARITY-OK') { Write-Output 'PASS: marker returned' } else { Write-Output 'FAIL: marker missing' }
    if ($exit -ne 0) { throw "apply_patch live turn exited $exit" }
} finally {
    Push-Location $RepoRoot
    Pop-Location
    $listeners = @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        if ($listener.OwningProcess) { Stop-Process -Id $listener.OwningProcess -Force -ErrorAction SilentlyContinue }
    }
    Write-Output ("Private parity repo: " + $repo)
}
