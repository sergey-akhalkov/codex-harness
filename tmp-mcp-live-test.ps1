[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$RepoRoot)
$ErrorActionPreference = 'Stop'
$registration = Get-Content -Raw 'C:\Users\noilw\.codex\harness\installation.json' | ConvertFrom-Json
$launcherPath = $registration.launcherSource
$repo = Join-Path ([IO.Path]::GetTempPath()) ('xai-mcp-' + [guid]::NewGuid().ToString('N').Substring(0, 8))
git init $repo | Out-Null
try {
    Push-Location $repo
    git config user.email 'mcp@example.invalid'
    git config user.name 'mcp'
    'initial' | Set-Content README.md
    git add README.md
    git commit -m init | Out-Null
    Pop-Location
    $out = Join-Path $repo 'out.txt'
    $err = Join-Path $repo 'err.txt'
    $ErrorActionPreference = 'Continue'
    & pwsh -NoProfile -File $launcherPath exec --profile xai -C $repo --dangerously-bypass-approvals-and-sandbox -c 'model_reasoning_effort="low"' "Use the nuphus desktop_screen_size MCP tool once to get this machine's screen resolution, then write only WIDTHxHEIGHT into mcp-proof.txt using apply_patch." 1> $out 2> $err
    $exit = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    Write-Output "mcp+patch turn exit: $exit"
    ((Get-Content -Raw $err) -split "`n" | Select-String -Pattern 'model:|provider:|mcp:|apply_patch|ERROR|warning') | Select-Object -First 14 | ForEach-Object { $_.Line }
    Get-Content $out -Tail 3
    $proof = Join-Path $repo 'mcp-proof.txt'
    if ((Test-Path $proof) -and ((Get-Content -Raw $proof).Trim() -match '^\d+x\d+$')) { Write-Output 'PASS: MCP result patched into file end-to-end' } else { Write-Output 'FAIL: mcp-proof missing or malformed' }
} finally {
    Push-Location $RepoRoot
    Pop-Location
    $listeners = @(Get-NetTCPConnection -LocalPort 56122 -State Listen -ErrorAction SilentlyContinue)
    foreach ($listener in $listeners) {
        if ($listener.OwningProcess) { Stop-Process -Id $listener.OwningProcess -Force -ErrorAction SilentlyContinue }
    }
    Write-Output ("Private mcp repo: " + $repo)
}
