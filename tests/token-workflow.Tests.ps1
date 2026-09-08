#requires -Version 7.4
# Native links and feature editor in owned homes. Dependency build is stubbed;
# the global acceptance probe separately exercises the actual compiled adapter.
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$repo=Split-Path $PSScriptRoot
$core=Import-Module (Join-Path $repo 'tools/kit.psm1') -Force -PassThru
$module=Import-Module (Join-Path $repo 'tools/token-workflow.psm1') -Force -PassThru
$original=(Get-Content (Join-Path $env:USERPROFILE '.codex/harness/installation.json') -Raw | ConvertFrom-Json).codexCommand
$root=Join-Path $env:TEMP ('harness-token-lifecycle-'+[guid]::NewGuid().ToString('N'))
$userRoot=Join-Path $root 'user'; $codexRoot=Join-Path $userRoot '.codex'
$oldPath=$env:PATH
$checks=0
function Assert-Token([bool]$value,[string]$message) { if(-not $value){throw $message};$script:checks++ }
function Invoke-Token([string]$mode) { Invoke-HarnessTokenWorkflow -SourceRoot $repo -CodexHome $codexRoot -UserHome $userRoot -CodexCommand $original -Mode $mode }
try {
    [void][IO.Directory]::CreateDirectory($root)
    $artifact=Join-Path $root 'dependency-stub.exe'
    [IO.File]::WriteAllText($artifact,'Not executable: lifecycle-only dependency fixture')
    & $module { param($path)
        $script:fixtureArtifact=$path
        function script:Get-TokenWorkflowBuild { param($SourceRoot,$CodexHome)
            @{adapter=$script:fixtureArtifact;rtk=$script:fixtureArtifact;sourceIdentity='fixture';version='0.48.0';binarySha256=(Get-FileHash $script:fixtureArtifact).Hash}
        }
    } $artifact
    Invoke-HarnessInstall -SourceRoot $repo -CodexHome $codexRoot -UserHome $userRoot -CodexCommand $original -PathScope Process | Out-Null
    Assert-Token (-not (Get-HarnessNativeFeatures $codexRoot $original).hooks) 'Fresh core enabled hooks'
    [IO.File]::AppendAllText((Join-Path $codexRoot 'config.toml'),"`n# foreign-preserved-17`n")
    Invoke-Token Install | Out-Null
    $features=Get-HarnessNativeFeatures $codexRoot $original
    Assert-Token ($features.hooks -and $features.code_mode) 'Accepted native features not enabled'
    Assert-Token ((Get-Item (Join-Path $codexRoot 'hooks.json')).ResolveLinkTarget($true).FullName -eq (Join-Path $repo 'global/rtk-hooks.json')) 'Wrong hook source'
    $configHash=(Get-FileHash (Join-Path $codexRoot 'config.toml')).Hash
    Invoke-Token Update | Out-Null
    Assert-Token ((Get-FileHash (Join-Path $codexRoot 'config.toml')).Hash -eq $configHash) 'Repeat activation changed config'
    Invoke-Token Check | Out-Null
    $relocated=Join-Path $root 'relocated checkout'
    $manifest=Import-PowerShellDataFile (Join-Path $repo 'global/kit.psd1')
    foreach($relative in @($manifest.RequiredFiles)+@('global/kit.psd1',$manifest.Profile,$manifest.Instructions)){
        $destination=Join-Path $relocated $relative
        [void][IO.Directory]::CreateDirectory((Split-Path $destination))
        Copy-Item -LiteralPath (Join-Path $repo $relative) -Destination $destination
    }
    foreach($relative in @($manifest.Skills,$manifest.Agents)){
        $destination=Join-Path $relocated $relative
        [void][IO.Directory]::CreateDirectory((Split-Path $destination))
        Copy-Item -LiteralPath (Join-Path $repo $relative) -Destination $destination -Recurse
    }
    $repo=$relocated
    Invoke-Token Update | Out-Null
    Assert-Token ((Get-Item (Join-Path $codexRoot 'hooks.json')).ResolveLinkTarget($true).FullName -eq (Join-Path $relocated 'global/rtk-hooks.json')) 'Relocation retained the old hook source'
    Set-HarnessNativeFeature $codexRoot $original 'hooks' $false
    Invoke-HarnessInstall -SourceRoot $repo -CodexHome $codexRoot -UserHome $userRoot -CodexCommand $original -PathScope Process | Out-Null
    Invoke-Token Update | Out-Null
    Assert-Token (-not (Get-HarnessNativeFeatures $codexRoot $original).hooks) 'Update overrode explicit suspension'
    $rtkLink=Join-Path $codexRoot 'harness/bin/rtk.exe'
    Remove-Item -LiteralPath $rtkLink
    Invoke-Token Update | Out-Null
    Assert-Token ((Get-Item $rtkLink).LinkType -eq 'SymbolicLink') 'Missing artifact link was not repaired'
    Remove-Item -LiteralPath $rtkLink
    [IO.File]::WriteAllText($rtkLink,'foreign')
    $caught=$false; try { Invoke-Token Update | Out-Null } catch { $caught=$_.Exception.Message -match 'target conflict' }
    Assert-Token ($caught -and (Get-Content $rtkLink -Raw) -eq 'foreign') 'Foreign artifact was not preserved'
    Remove-Item -LiteralPath $rtkLink
    Invoke-Token Update | Out-Null
    # Fail after owned link mutations to exercise recorded recovery.
    & $module { function script:Set-HarnessNativeFeature { throw 'injected-feature-failure' } }
    $caught=$false; try { Invoke-Token Disconnect | Out-Null } catch { $caught=$_.Exception.Message -match 'injected-feature-failure' }
    Assert-Token ($caught -and (Test-Path (Join-Path $codexRoot 'harness/token-workflow-pending.json'))) 'Failure did not retain recovery evidence'
    & $module { function script:Set-HarnessNativeFeature { param($CodexHome,$CodexCommand,$Feature,$Enabled) kit\Set-HarnessNativeFeature $CodexHome $CodexCommand $Feature $Enabled } }
    Invoke-Token Recover | Out-Null
    Invoke-Token Check | Out-Null
    Assert-Token (-not (Test-Path (Join-Path $codexRoot 'harness/token-workflow-pending.json'))) 'Recovery remained pending'
    Invoke-Token Disconnect | Out-Null
    $features=Get-HarnessNativeFeatures $codexRoot $original
    Assert-Token (-not $features.hooks -and -not $features.code_mode) 'Disconnect did not suspend owned selection'
    Assert-Token (-not (Test-Path $rtkLink)) 'Owned binary registration survived disconnect'
    Assert-Token ((Get-Content (Join-Path $codexRoot 'config.toml') -Raw).Contains('foreign-preserved-17')) 'Foreign config lost'
    Assert-Token ((Get-Content (Join-Path $codexRoot 'hooks.json') -Raw).Trim() -eq '{"hooks":{}}') 'Rejected hooks returned'
    # A preexisting Code Mode selection is preserved on detach.
    Set-HarnessNativeFeature $codexRoot $original 'code_mode' $true
    Invoke-Token Install | Out-Null
    Invoke-Token Disconnect | Out-Null
    Assert-Token ((Get-HarnessNativeFeatures $codexRoot $original).code_mode) 'Preexisting Code Mode was disabled'
    [pscustomobject]@{passed=$true;assertions=$checks;evidence=$root}
} finally { $env:PATH=$oldPath }
