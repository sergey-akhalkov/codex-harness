#requires -Version 7.4
# Deliberately no param block: named/positional native CLI arguments belong in
# $args, without PowerShell parameter binding or command-line reconstruction.
$harnessPreviousErrorAction = $ErrorActionPreference
# Keep this installed bootstrap independent of the checkout and its modules.
# Only preparation can fall back; an upstream process is launched exactly once.
$harnessSavedRegistration = $null
$harnessSource = $PSCommandPath
if (Get-Variable -Name CodexHarnessLauncherActive -Scope Global -ValueOnly -ErrorAction SilentlyContinue) {
    [Console]::Error.WriteLine('codex-harness: Recursive launcher registration; repair the original Codex CLI path.')
    exit 1
}
try {
    $ErrorActionPreference = 'Stop'
    $harnessEntry = Get-Item -LiteralPath $PSCommandPath -Force
    $harnessSource = if ($harnessEntry.LinkType) { $harnessEntry.ResolveLinkTarget($true).FullName } else { $harnessEntry.FullName }
    $harnessDirectory = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
    $harnessMetadata = Join-Path $harnessDirectory 'harness/installation.json'
    if ((Get-Item -LiteralPath $harnessMetadata -ErrorAction Stop).Length -gt 65536) { throw 'Harness registration exceeds its bound.' }
    $harnessSavedRegistration = Get-Content -LiteralPath $harnessMetadata -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    Import-Module (Join-Path $harnessSavedRegistration.sourceRoot 'tools/launcher.psm1') -Scope Local -Force
    $harnessRegistration = Get-HarnessLaunchConfiguration -LauncherSource $harnessSource
    try { [string[]] $harnessTaskArguments = Get-HarnessTaskArguments -Arguments $args }
    catch {
        # Invalid explicit harness options are user input, not an unavailable
        # integration. Preserve their validation without launching a session.
        if ($args.Count -and $args[0] -cmatch '^--harness-effort(?:=|$)') {
            [Console]::Error.WriteLine("codex-harness: $($_.Exception.Message)")
            exit 1
        }
        throw
    }
    [string[]] $harnessArguments = Get-HarnessLaunchArguments -Registration $harnessRegistration -Arguments $harnessTaskArguments
    $harnessRoots = @(Get-HarnessAdditionalRoots -Arguments $harnessTaskArguments)
} catch {
    $harnessCandidates = [Collections.Generic.List[string]]::new()
    if ($harnessSavedRegistration -and $harnessSavedRegistration.PSObject.Properties['codexCommand']) {
        $harnessCandidates.Add([string]$harnessSavedRegistration.codexCommand)
    }
    foreach ($harnessCandidate in @(Get-Command codex -All -CommandType Application,ExternalScript -ErrorAction SilentlyContinue)) {
        $harnessCandidates.Add($harnessCandidate.Source)
    }
    $harnessOriginal = $null
    foreach ($harnessCandidate in $harnessCandidates) {
        try {
            if (-not [IO.Path]::IsPathFullyQualified($harnessCandidate) -or [IO.Path]::GetExtension($harnessCandidate) -notin @('.exe', '.ps1')) { continue }
            $harnessItem = Get-Item -LiteralPath $harnessCandidate -Force -ErrorAction Stop
            if ($harnessItem.PSIsContainer) { continue }
            $harnessResolved = if ($harnessItem.LinkType) { $harnessItem.ResolveLinkTarget($true).FullName } else { $harnessItem.FullName }
            if ([string]::Equals($harnessResolved, $harnessSource, [StringComparison]::OrdinalIgnoreCase) -or
                [string]::Equals($harnessResolved, $PSCommandPath, [StringComparison]::OrdinalIgnoreCase)) { continue }
            if ([IO.Path]::GetExtension($harnessResolved) -eq '.ps1') {
                # Other installations of this same bootstrap cannot be upstream.
                $harnessReader = [IO.File]::OpenText($harnessResolved)
                try {
                    $harnessHeader = [char[]]::new(65536)
                    $harnessRead = $harnessReader.ReadBlock($harnessHeader, 0, $harnessHeader.Length)
                    if ([string]::new($harnessHeader, 0, $harnessRead).Contains('CodexHarnessLauncherActive')) { continue }
                } finally { $harnessReader.Dispose() }
            }
            $harnessOriginal = $harnessCandidate
            break
        } catch { continue }
    }
    if (-not $harnessOriginal) {
        [Console]::Error.WriteLine('codex-harness: Harness unavailable and the original Codex CLI could not be found. Restore its installation or PATH; no command was launched.')
        exit 1
    }
    [Console]::Error.WriteLine('codex-harness: Harness unavailable; starting ordinary Codex CLI with your original arguments and local settings.')
    $harnessRegistration = [pscustomobject]@{ codexCommand = $harnessOriginal }
    [string[]]$harnessArguments = $args
    $harnessRoots = @()
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
