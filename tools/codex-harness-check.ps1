#requires -Version 7.4
[CmdletBinding()]
param([string]$ProjectPath=(Get-Location).Path, [string]$CodexHome,
    [string]$UserHome=[Environment]::GetFolderPath('UserProfile'), [switch]$Json)
$ErrorActionPreference='Stop'
$entry=Get-Item -LiteralPath $PSCommandPath -Force
$source=if ($entry.LinkType) {$entry.ResolveLinkTarget($true).FullName} else {$entry.FullName}
$root=Split-Path (Split-Path $source -Parent) -Parent
$result=& (Join-Path $root 'install.ps1') -Mode Check -Diagnose -ProjectPath $ProjectPath -CodexHome $CodexHome -UserHome $UserHome
if ($Json) { $result | ConvertTo-Json -Depth 25 } else { $result }
