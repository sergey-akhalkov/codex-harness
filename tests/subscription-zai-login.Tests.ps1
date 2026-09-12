#requires -Version 7.4
# Isolated Z.AI key login. Never uses live CODEX_HOME or stock ocx login.
[CmdletBinding()]
param()
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot
$root = Join-Path ([IO.Path]::GetTempPath()) ('codex-subscription-zai-login-' + [guid]::NewGuid().ToString('N'))
$source = Join-Path $root 'source'
$user = Join-Path $root 'user'
$codex = Join-Path $root 'codex'
[void][IO.Directory]::CreateDirectory((Join-Path $source 'global/opencodex/agents'))
[void][IO.Directory]::CreateDirectory($codex)
[void][IO.Directory]::CreateDirectory($user)
Copy-Item -LiteralPath (Join-Path $repository 'global/opencodex/config.json') -Destination (Join-Path $source 'global/opencodex/config.json')
Copy-Item -LiteralPath (Join-Path $repository 'global/opencodex/agents/middle.toml') -Destination (Join-Path $source 'global/opencodex/agents/middle.toml')
$profile = Join-Path $codex 'zai.config.toml'
$catalog = Join-Path $codex 'zai.models.json'
[IO.File]::WriteAllText($profile, "model = 'glm-5.3'`n")
[IO.File]::WriteAllText($catalog, '{"models":[{"slug":"glm-5.3"}]}')
$beforeProfile = Get-FileHash -LiteralPath $profile
$beforeCatalog = Get-FileHash -LiteralPath $catalog
$keyFile = Join-Path $root 'incoming-key.txt'
[IO.File]::WriteAllText($keyFile, 'synthetic-zai-secret-must-not-print')
$login = Join-Path $repository 'tools/opencodex-zai-login.ps1'
$output = & $login -CodexHome $codex -UserHome $user -SourceRoot $source -KeyFile $keyFile -NoOpenBrowser | Out-String
if ($output -match 'synthetic-zai-secret-must-not-print') { throw 'Login printed the key.' }
$store = Join-Path $codex 'harness/subscriptions/zai-key.txt'
if (-not (Test-Path -LiteralPath $store)) { throw 'Private key store was not created.' }
if ((Get-Content -LiteralPath $store -Raw).Trim() -cne 'synthetic-zai-secret-must-not-print') { throw 'Stored key did not match the supplied value.' }
if ((Get-FileHash -LiteralPath $profile).Hash -cne $beforeProfile.Hash) { throw 'Login modified zai.config.toml.' }
if ((Get-FileHash -LiteralPath $catalog).Hash -cne $beforeCatalog.Hash) { throw 'Login modified zai.models.json.' }
$linked = Join-Path $source 'global/opencodex/config.json'
if ((Get-Content -LiteralPath $linked -Raw) -match 'synthetic-zai-secret-must-not-print') { throw 'Login wrote the key into source config.' }
'PASS isolated zai login stores the key privately and leaves the local profile unchanged'
$resolved = [IO.Path]::GetFullPath($root)
if ((Split-Path $resolved) -ine [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') -or (Split-Path $resolved -Leaf) -notlike 'codex-subscription-zai-login-*') { throw 'Refusing fixture cleanup outside the explicit temporary workspace.' }
Remove-Item -LiteralPath $resolved -Recurse -Force

