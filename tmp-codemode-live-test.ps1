[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$RepoRoot)
$ErrorActionPreference = 'Stop'
$registration = Get-Content -Raw 'C:\Users\noilw\.codex\harness\installation.json' | ConvertFrom-Json
$launcherPath = $registration.launcherSource
$repo = Join-Path ([IO.Path]::GetTempPath()) ('xai-cm-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
git init $repo | Out-Null
try {
    Push-Location $repo
    git config user.email 'cm@example.invalid'
    git config user.name 'cm'
    'initial' | Set-Content README.md
    git add README.md
    git commit -m init | Out-Null
    Pop-Location
    $out = Join-Path $repo 'out.txt'
    $err = Join-Path $repo 'err.txt'
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec --profile xai -C $repo --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' "Use the exec tool (Code Mode) to compute 6*7 and write the result into cm-proof.txt via the text() helper. Then print the file content." 1> $out 2> $err
    $exit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "code_mode exec turn exit: $exit"
    ((Get-Content -Raw $err) -split "`n" | Select-String -Pattern 'model:|provider:|exec|ERROR|warning') | Select-Object -First 12 | ForEach-Object { $_.Line }
    $proof = Join-Path $repo 'cm-proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -eq '42')) { Write-Output 'PASS: code_mode exec tool worked through the shim' } else { Write-Output 'FAIL: cm-proof missing (check whether exec was called at all)' }
} finally {
    Push-Location $RepoRoot
    Pop-Location
    $listeners = @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        if ($listener.OwningProcess) { Stop-Process -Id $listener.OwningProcess -Force -ErrorAction SilentlyContinue }
    }
    Write-Output ("Private cm repo: " + $repo)
}
