# Windows CLI example

PowerShell 7 and Git suffice; no desktop app or Handoff is required. This example assumes a task owns `src/parser.py` and the required untracked input `fixtures/input.txt`; substitute actual scoped paths and project checks. Review each command's outcome before proceeding. Commits are optional and require task authorization; this path uses patches instead.

```powershell
$repo = (git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE) { throw 'Not a Git checkout' }
$base = (git -C $repo rev-parse HEAD).Trim() # Choose a different explicit base if required.
$taskId = [guid]::NewGuid().ToString('N')
$branch = "task/$taskId"
$tree = Join-Path (Split-Path $repo -Parent) "task-$taskId"
$evidence = Join-Path ([IO.Path]::GetTempPath()) "worktree-$taskId"
git -C $repo status --short --untracked-files=all
git -C $repo worktree list --porcelain
if (Test-Path -LiteralPath $tree) { throw 'Path collision' }
git -C $repo show-ref --verify --quiet "refs/heads/$branch"
if ($LASTEXITCODE -eq 0) { throw 'Branch collision' }
if ($LASTEXITCODE -ne 1) { throw 'Branch lookup failed' }
New-Item -ItemType Directory -Path $evidence | Out-Null
$inputs = Join-Path $evidence 'inputs.patch'
git -C $repo diff --binary "--output=$inputs" $base -- src/parser.py
if ($LASTEXITCODE) { throw 'Input snapshot failed' }
git -C $repo worktree add -b $branch $tree $base
if ($LASTEXITCODE) { throw 'Worktree creation failed; inspect owned resources' }
if ((Get-Item -LiteralPath $inputs).Length) {
    git -C $tree apply --check $inputs
    if ($LASTEXITCODE) { throw 'Input patch does not apply' }
    git -C $tree apply $inputs
    if ($LASTEXITCODE) { throw 'Input patch failed' }
}
New-Item -ItemType Directory -Path (Join-Path $tree 'fixtures') -Force | Out-Null
$targetInput = Join-Path $tree 'fixtures/input.txt'
if (Test-Path -LiteralPath $targetInput) { throw 'Input target already exists' }
Copy-Item -LiteralPath (Join-Path $repo 'fixtures/input.txt') -Destination $targetInput
if ((Get-FileHash -LiteralPath (Join-Path $repo 'fixtures/input.txt')).Hash -ne
    (Get-FileHash -LiteralPath $targetInput).Hash) { throw 'Untracked input mismatch' }
$observedInputs = Join-Path $evidence 'observed-inputs.patch'
git -C $tree diff --binary "--output=$observedInputs" $base -- src/parser.py
if ($LASTEXITCODE -or (Get-FileHash $inputs).Hash -ne (Get-FileHash $observedInputs).Hash) {
    throw 'Tracked input mismatch'
}
git -C $tree rev-parse --show-toplevel
git -C $tree status --short --untracked-files=all
```

For deleted, renamed or additional required files, extend the explicit snapshot and compare those paths too. Do not copy ignored credentials/build caches as inputs. Preserve the patch and per-file identities for handoff. Now activate navigation tools for `$tree`, refresh applicable indexes, discover and execute the project's native setup/checks there, implement the owned task, and check the candidate.

To integrate a tracked-file change without replaying transferred dirty input, compute the delta against the original input snapshot. Git's temporary index can hold the transferred baseline without changing the real index:

```powershell
$priorIndex = $env:GIT_INDEX_FILE
try {
    $env:GIT_INDEX_FILE = Join-Path $evidence 'baseline.index'
    git -C $tree read-tree $base
    if ($LASTEXITCODE) { throw 'Baseline index failed' }
    if ((Get-Item -LiteralPath $inputs).Length) {
        git -C $tree apply --cached $inputs
        if ($LASTEXITCODE) { throw 'Baseline patch failed' }
    }
    $candidate = Join-Path $evidence 'candidate.patch'
    git -C $tree diff --binary "--output=$candidate" -- src/parser.py
    if ($LASTEXITCODE) { throw 'Candidate diff failed' }
} finally { $env:GIT_INDEX_FILE = $priorIndex }
Get-Content -LiteralPath $candidate
git -C $repo apply --check $candidate
if ($LASTEXITCODE) { throw 'Integration conflict; preserve both trees and resolve explicitly' }
git -C $repo apply $candidate
if ($LASTEXITCODE) { throw 'Integration failed' }
git -C $repo diff -- src/parser.py
```

New files need explicit reviewed transfer with destination-collision checks; deleted/renamed files need their intended operation verified. Run the project's applicable checks in `$repo` on the combined result. If a check fails, retain both trees and correct the integration; do not declare completion.

Before cleanup, inspect `git -C $tree status --short --untracked-files=all`, `git -C $tree ls-files --others --ignored --exclude-standard`, and `git -C $repo log HEAD..$branch`. For this uncommitted patch workflow the tree remains dirty: retain it until its artifacts are preserved and deliberate cleanup is authorized. Do not force removal just to complete this example. Once clean, all work is preserved and removal is authorized:

```powershell
git -C $repo worktree remove $tree
if ($LASTEXITCODE) { throw 'Cleanup refused; retain tree and inspect' }
git -C $repo branch -d $branch
if ($LASTEXITCODE) { throw 'Branch retained; inspect unmerged work' }
```
