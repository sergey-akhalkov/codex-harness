#requires -Version 7.4
# Deliberately no param block: named/positional native CLI arguments belong in
# $args, without PowerShell parameter binding or command-line reconstruction.
$harnessPreviousErrorAction = $ErrorActionPreference
try {
    $ErrorActionPreference = 'Stop'
    if (Get-Variable -Name CodexHarnessLauncherActive -Scope Global -ValueOnly -ErrorAction SilentlyContinue) {
        throw 'Recursive Codex harness invocation detected. Run install.ps1 to repair the original CLI registration.'
    }
    $harnessEntry = Get-Item -LiteralPath $PSCommandPath -Force
    $harnessSource = if ($harnessEntry.LinkType) { $harnessEntry.ResolveLinkTarget($true).FullName } else { $harnessEntry.FullName }
    Import-Module (Join-Path (Split-Path $harnessSource -Parent) 'launcher.psm1') -Scope Local -Force
    $harnessRegistration = Get-HarnessLaunchConfiguration -LauncherSource $harnessSource
    [string[]] $harnessTaskArguments = Get-HarnessTaskArguments -Arguments $args
    [string[]] $harnessArguments = @(Get-HarnessArguments -Arguments $harnessTaskArguments -ProfileName $harnessRegistration.profileName)
    $harnessRoots = @(Get-HarnessAdditionalRoots -Arguments $harnessTaskArguments)
} catch {
    [Console]::Error.WriteLine("codex-harness: $($_.Exception.Message)")
    exit 1
} finally {
    $ErrorActionPreference = $harnessPreviousErrorAction
}

# Scope the recursion guard to this PowerShell process; an inherited environment
# guard would incorrectly block intentional Codex calls from a child session.
$global:CodexHarnessLauncherActive = $true
$PSNativeCommandUseErrorActionPreference = $false
$harnessPreviousRoots = $env:HARNESS_LSP_WORKSPACE_ROOTS
try {
    $env:HARNESS_LSP_WORKSPACE_ROOTS = if ($harnessRoots.Count) { ConvertTo-Json -InputObject $harnessRoots -Compress } else { $null }
    if ($MyInvocation.ExpectingInput) {
        $input | & $harnessRegistration.codexCommand @harnessArguments
    } else {
        & $harnessRegistration.codexCommand @harnessArguments
    }
    $harnessExitCode = $LASTEXITCODE
} finally {
    $env:HARNESS_LSP_WORKSPACE_ROOTS = $harnessPreviousRoots
    Remove-Variable -Name CodexHarnessLauncherActive -Scope Global -ErrorAction SilentlyContinue
}
exit $harnessExitCode
