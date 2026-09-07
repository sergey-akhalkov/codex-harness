#requires -Version 7.4
# Real native reads in isolated homes; no model requests or live service lifecycle.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$NativeCodex, [switch]$KeepFixture)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot -Parent
$fixture=Join-Path $env:TEMP ('harness-source-tests-'+[guid]::NewGuid().ToString('N'))
$testHome=Join-Path $fixture 'user/.codex'
$testUser=Join-Path $fixture 'user'
$project=Join-Path $fixture 'consumer project'
$oldPath=$env:PATH
$count=0
function Assert-Source([bool]$Condition,[string]$Message) {
    if (-not $Condition) { throw $Message }
    $script:count++
    Write-Output "PASS: $Message"
}
function Write-SourceFixture([string]$Path,[string]$Body) {
    $null=New-Item -ItemType Directory -Path (Split-Path $Path) -Force
    [IO.File]::WriteAllText($Path,$Body,[Text.UTF8Encoding]::new($false))
}
function Read-SourceReport {
    & (Join-Path $repo 'install.ps1') -Mode Check -Diagnose -CodexHome $testHome -UserHome $testUser -ProjectPath $project -CodexCommand $NativeCodex
}
function Remove-SourceFixture([string]$Path) {
    $full=[IO.Path]::GetFullPath($Path)
    if ($full -ne $fixture -and -not $full.StartsWith($fixture+'\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Cleanup escaped fixture' }
    $item=Get-Item -LiteralPath $full -Force -ErrorAction SilentlyContinue
    if (-not $item) { return }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { Remove-Item -LiteralPath $full -Force; return }
    if ($item.PSIsContainer) { foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-SourceFixture $child.FullName } }
    Remove-Item -LiteralPath $full -Force
}
try {
    $null=New-Item -ItemType Directory -Path $testHome,$project -Force
    & git init --quiet $project
    if ($LASTEXITCODE) { throw 'git init failed' }
    $kit=Import-Module (Join-Path $repo 'tools/kit.psm1') -Force -PassThru
    & $kit {
        function script:Assert-HarnessPrerequisites { @{codex='fixture';openspec='fixture';powershell='fixture'} }
        function script:Test-HarnessRuntime { @{status='stub; native reads tested separately'} }
    }
    $null=Invoke-HarnessInstall -SourceRoot $repo -CodexHome $testHome -UserHome $testUser -CodexCommand $NativeCodex -PathScope Process
    $trusted="[projects.'$project']`ntrust_level = 'trusted'`n[features]`nhooks = true`n"
    Write-SourceFixture "$testHome/config.toml" $trusted
    $hookMarker=Join-Path $fixture 'hook-ran'
    $hookCode="[IO.File]::WriteAllText('$hookMarker','unexpected hook execution')"
    $hookCommand='pwsh -NoProfile -EncodedCommand '+[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($hookCode))
    Write-SourceFixture "$testHome/hooks.json" (@{hooks=@{SessionStart=@(@{hooks=@(@{type='command';command=$hookCommand;timeout=5})})}} | ConvertTo-Json -Depth 8)
    Write-SourceFixture "$testHome/auth-sentinel" 'unrelated-auth'
    Write-SourceFixture "$testHome/history.jsonl" 'unrelated-history'
    $before=@{}
    foreach ($file in @('config.toml','auth-sentinel','history.jsonl')) { $before[$file]=(Get-FileHash -LiteralPath "$testHome/$file").Hash }
    $clean=Read-SourceReport
    Assert-Source ($clean.native.status -eq 'observed') 'real native source inspection completes'
    Assert-Source ($clean.status -eq 'healthy') 'clean fixture has no findings'
    Assert-Source (-not (Test-Path -LiteralPath $hookMarker)) 'native source reads do not execute SessionStart hooks'
    $reasoning=@($clean.settings | Where-Object key -eq 'model_reasoning_effort')[0]
    Assert-Source ($reasoning.origin.profile -eq 'harness' -and $reasoning.value -eq 'xhigh') 'profile reasoning and source are identified'
    Assert-Source ($clean.freshness.existingSessions -eq 'unknown' -and $clean.freshness.existingMcpLspServers -eq 'unknown') 'existing process freshness is not inferred'
    foreach ($file in $before.Keys) { Assert-Source ((Get-FileHash -LiteralPath "$testHome/$file").Hash -eq $before[$file]) "native reads preserve $file" }

    $secret='DIAGNOSTIC_PRIVATE_SENTINEL_9b38'
    Write-SourceFixture "$project/.codex/config.toml" "model_reasoning_effort = 'low'`ndeveloper_instructions = '$secret'`n[features]`nhooks = false`n"
    Write-SourceFixture "$project/.agents/skills/duplicate/SKILL.md" "---`nname: openspec-apply-change`ndescription: $secret`n---`n$secret"
    Write-SourceFixture "$fixture/alternate.md" 'different instructions'
    Remove-Item -LiteralPath "$testHome/AGENTS.md"
    $null=New-Item -ItemType SymbolicLink -Path "$testHome/AGENTS.md" -Target "$fixture/alternate.md"
    $broken=Read-SourceReport
    Assert-Source ($broken.status -eq 'attention') 'simultaneous findings retain successful native evidence'
    foreach ($code in @('setting-overridden','skill-name-collision','link-retargeted')) {
        Assert-Source (@($broken.findings | Where-Object code -eq $code).Count -gt 0) "one report includes $code"
    }
    $reasoning=@($broken.settings | Where-Object key -eq 'model_reasoning_effort')[0]
    Assert-Source ($reasoning.value -eq 'low' -and $reasoning.origin.type -eq 'project') 'trusted project wins with exact native source'
    Assert-Source (-not ($broken | ConvertTo-Json -Depth 25).Contains($secret)) 'prompt bodies and skill descriptions are withheld'
    $hooks=@($broken.settings | Where-Object key -eq 'features.hooks')[0]
    Assert-Source ($hooks.value -is [bool] -and $hooks.value -eq $false -and $hooks.origin.type -eq 'project') 'explicit false remains a declaration'

    Remove-Item -LiteralPath "$testHome/AGENTS.md"
    $null=New-Item -ItemType SymbolicLink -Path "$testHome/AGENTS.md" -Target (Join-Path $repo 'global/principles-of-work.md')
    Remove-Item -LiteralPath "$project/.agents/skills/duplicate/SKILL.md"
    Remove-Item -LiteralPath "$project/.codex/config.toml"
    $restored=Read-SourceReport
    Assert-Source ($restored.status -eq 'healthy') 'restoring sources clears all three findings'

    Write-SourceFixture "$project/.codex/config.toml" "model_reasoning_effort = 'low'`n"
    Write-SourceFixture "$testHome/config.toml" "[projects.'$project']`ntrust_level = 'untrusted'`n"
    $untrusted=Read-SourceReport
    $reasoning=@($untrusted.settings | Where-Object key -eq 'model_reasoning_effort')[0]
    Assert-Source ($reasoning.value -eq 'xhigh' -and -not $reasoning.overridden) 'untrusted project does not override profile'
    Assert-Source (@($untrusted.layers | Where-Object status -eq 'disabled').Count -gt 0) 'disabled project layer is distinct from an override'

    # Replace the fixture link before editing; never write through to reusable source.
    $profileText=Get-Content -LiteralPath "$testHome/harness.config.toml" -Raw
    Remove-Item -LiteralPath "$testHome/harness.config.toml"
    Write-SourceFixture "$testHome/harness.config.toml" ($profileText+"`n[projects.'$project']`ntrust_level='trusted'`n")
    $context=Read-SourceReport
    Assert-Source ($context.status -eq 'incomplete' -and @($context.findings | Where-Object code -eq 'profile-context-unresolved').Count -eq 1) 'profile trust changes cannot masquerade as native effective evidence'
    Assert-Source (@($context.settings | Where-Object { $null -ne $_.origin }).Count -eq 0) 'uncertain context withholds effective origins'
    Write-SourceFixture "$testHome/harness.config.toml" $profileText

    Write-SourceFixture "$testHome/config.toml" "this is invalid $secret"
    $invalid=Read-SourceReport
    Assert-Source ($invalid.status -eq 'incomplete') 'invalid native config returns incomplete'
    Assert-Source (-not ($invalid | ConvertTo-Json -Depth 25).Contains($secret)) 'invalid config diagnostics do not disclose parser input'
    Assert-Source ($invalid.links.Count -gt 0) 'native failure retains independent link evidence'
    Write-Output "PASS: $count source diagnostic assertions. Fixture: $fixture"
} finally {
    $env:PATH=$oldPath
    if (-not $KeepFixture) { Remove-SourceFixture $fixture }
}
