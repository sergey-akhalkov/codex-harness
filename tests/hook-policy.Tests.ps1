#requires -Version 7.4
# Native config editor, owned homes only. No models, packages or global writes.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot
$kit = Import-Module (Join-Path $repo 'tools/kit.psm1') -Force -PassThru
$original = (Get-Content (Join-Path $env:USERPROFILE '.codex/harness/installation.json') -Raw | ConvertFrom-Json).codexCommand
$pythonExe = Join-Path $env:USERPROFILE 'AppData/Roaming/uv/tools/serena-agent/Scripts/python.exe'
$root = Join-Path $env:TEMP ('harness-hook-policy-' + [guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
$fixtures = @('', '# comment without newline', "model = 'gpt-6-astra'`n[features]`nhooks = true`nweb_search = false`n",
    "features = { hooks = true, web_search = false }`nmarker = '''line one`n[features]`nhooks = true`nline end'''`n")
$checks = 0
try {
    foreach ($fixture in $fixtures) {
        $homePath = Join-Path $root ('case-' + $checks)
        [void][IO.Directory]::CreateDirectory($homePath)
        $configPath = Join-Path $homePath 'config.toml'
        [IO.File]::WriteAllText($configPath, $fixture)
        $beforePath = Join-Path $homePath 'before.toml'
        [IO.File]::WriteAllText($beforePath, $fixture)
        & $kit { param($h,$exe) Disable-HarnessHooks $h $exe } $homePath $original
        $verify = @'
import sys,tomllib
from pathlib import Path
before,after = (tomllib.loads(Path(p).read_text(encoding='utf-8-sig')) for p in sys.argv[1:])
assert after['features']['hooks'] is False
for value in (before,after):
    value.get('features',{}).pop('hooks',None)
    if value.get('features') == {}: value.pop('features')
assert before == after, 'Native hook editor changed unrelated settings'
'@
        & $pythonExe -B -c $verify $beforePath $configPath
        if ($LASTEXITCODE) { throw 'Semantic preservation check failed' }
        $hash = (Get-FileHash $configPath).Hash
        & $kit { param($h,$exe) Disable-HarnessHooks $h $exe } $homePath $original
        if ((Get-FileHash $configPath).Hash -ne $hash) { throw 'Repeat policy application changed bytes' }
        $checks++
    }
    $invalidHome = Join-Path $root 'invalid'
    [void][IO.Directory]::CreateDirectory($invalidHome)
    $invalid = Join-Path $invalidHome 'config.toml'
    [IO.File]::WriteAllText($invalid, '[broken')
    $hash = (Get-FileHash $invalid).Hash
    $failed = $false
    try { & $kit { param($h,$exe) Disable-HarnessHooks $h $exe } $invalidHome $original } catch { $failed = $true }
    if (-not $failed -or (Get-FileHash $invalid).Hash -ne $hash) { throw 'Invalid config was not preserved' }
    [pscustomobject]@{ passed = $true; semanticAndIdempotenceCases = $checks; invalidConfigPreserved = $true; evidence = $root }
} finally { Remove-Module $kit }
