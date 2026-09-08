#requires -Version 7.4
# Runs the documented blocks in a new owned Git fixture; retains all artifacts.
$ErrorActionPreference = 'Stop'
$examplePath = Join-Path $PSScriptRoot '../.agents/skills/isolated-worktree/references/windows.md'
$blocks = [regex]::Matches([IO.File]::ReadAllText($examplePath), '(?s)```powershell\r?\n(.*?)```')
if ($blocks.Count -ne 3) { throw 'Expected creation, integration and guarded cleanup examples' }
$fixtureRoot = Join-Path ([IO.Path]::GetTempPath()) ('worktree-example-' + [guid]::NewGuid().ToString('N'))
$fixtureRepo = Join-Path $fixtureRoot 'repo'
New-Item -ItemType Directory -Path (Join-Path $fixtureRepo 'src') -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $fixtureRepo 'fixtures') | Out-Null
git init -q $fixtureRepo
if ($LASTEXITCODE) { throw 'Fixture init failed' }
[IO.File]::WriteAllText((Join-Path $fixtureRepo 'src/parser.py'), "value = 1`n")
[IO.File]::WriteAllText((Join-Path $fixtureRepo 'unrelated.txt'), "original`n")
git -C $fixtureRepo add -- src/parser.py unrelated.txt
git -C $fixtureRepo -c user.name=Fixture -c user.email=fixture@example.invalid commit -qm fixture
if ($LASTEXITCODE) { throw 'Fixture commit failed' }
[IO.File]::WriteAllText((Join-Path $fixtureRepo 'src/parser.py'), "value = 2`n")
[IO.File]::WriteAllText((Join-Path $fixtureRepo 'fixtures/input.txt'), "needed untracked input`n")
[IO.File]::WriteAllText((Join-Path $fixtureRepo 'unrelated.txt'), "unrelated dirty work`n")
$unrelatedHash = (Get-FileHash -LiteralPath (Join-Path $fixtureRepo 'unrelated.txt')).Hash
Push-Location $fixtureRepo
try {
    . ([scriptblock]::Create($blocks[0].Groups[1].Value))
    if ([IO.File]::ReadAllText((Join-Path $tree 'src/parser.py')).Replace("`r`n", "`n") -cne "value = 2`n") { throw 'Dirty tracked input omitted' }
    if ([IO.File]::ReadAllText((Join-Path $tree 'fixtures/input.txt')) -cne "needed untracked input`n") { throw 'Untracked input omitted' }
    if (-not (Test-Path -LiteralPath (Join-Path $tree '.git') -PathType Leaf)) { throw 'Linked Git metadata not exercised' }
    [IO.File]::WriteAllText((Join-Path $tree 'src/parser.py'), "value = 3`n")
    . ([scriptblock]::Create($blocks[1].Groups[1].Value))
    if ([IO.File]::ReadAllText((Join-Path $fixtureRepo 'src/parser.py')).Replace("`r`n", "`n") -cne "value = 3`n") { throw 'Integrated result wrong' }
    if ((Get-FileHash -LiteralPath (Join-Path $fixtureRepo 'unrelated.txt')).Hash -ne $unrelatedHash) { throw 'Unrelated input changed' }
    # The documentation explicitly retains this dirty tree; verify Git refuses removal.
    git -C $fixtureRepo worktree remove $tree 2>$null
    if ($LASTEXITCODE -eq 0 -or -not (Test-Path -LiteralPath $tree)) { throw 'Unsaved tree was removed' }
    $receipt = @{ status = 'passed'; fixture = $fixtureRoot; worktree = $tree; evidence = $evidence;
        checked = @('documented creation', 'tracked and untracked input transfer', 'linked Git metadata',
            'documented candidate-only integration', 'independent integrated content', 'unrelated dirty preservation', 'dirty cleanup refusal') }
    $receipt | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $fixtureRoot 'result.json') -Encoding utf8
    $receipt | ConvertTo-Json -Depth 5
} finally { Pop-Location }
