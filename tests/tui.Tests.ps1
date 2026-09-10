#requires -Version 7.4
# Actual Windows TUI smoke test using a headless native pseudoconsole.
# No model request is submitted. Uses live auth via a temporary link if present.
[CmdletBinding()]
param([string] $CodexCommand, [string] $ConfigBridge, [switch] $KeepFailureArtifact)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if (-not $IsWindows) { throw 'This native terminal test requires Windows.' }
if (-not ('Harness.Tests.ConPty' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'ConPty.cs') }
$repository = Split-Path $PSScriptRoot -Parent
if (-not $ConfigBridge) { $ConfigBridge = Join-Path $repository 'target/debug/codex-harness.exe' }
if (-not (Test-Path -LiteralPath $ConfigBridge)) { throw 'Build codex-harness before running this test.' }
$hostCodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE '.codex' }
if (-not $CodexCommand) {
    $hostRegistration = Join-Path $hostCodexHome 'harness/installation.json'
    if (Test-Path -LiteralPath $hostRegistration) { $CodexCommand = (Get-Content -LiteralPath $hostRegistration -Raw | ConvertFrom-Json).codexCommand }
    else { $CodexCommand = (Get-Command codex.ps1 -CommandType ExternalScript | Select-Object -First 1).Source }
}
$pwsh = (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ('codex-tui проверка ' + [guid]::NewGuid().ToString('N'))
$previousCodexHome = $env:CODEX_HOME
$previousPath = $env:PATH
$terminal = $null
$links = @()
$completed = $false
$sharedProfile = Join-Path $repository 'global/harness.config.toml'
$sharedProfileHash = (Get-FileHash -LiteralPath $sharedProfile -Algorithm SHA256).Hash
function Wait-Terminal([string] $Pattern, [int] $Seconds = 20) {
    $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        if ($terminal.Transcript -match $Pattern -or (Get-TerminalText) -match $Pattern) { return }
        if ($terminal.HasExited) { throw "TUI exited before '$Pattern' (exit $($terminal.ExitCode))." }
        Start-Sleep -Milliseconds 100
    }
    # Keep the full terminal transcript only in the disposable test directory;
    # it can contain account display information and must not enter repo logs.
    $terminal.Transcript | Set-Content -LiteralPath (Join-Path $temporaryRoot 'failure-transcript.txt') -Encoding utf8
    $summary = ((Get-TerminalText) -split "`n" | Where-Object { $_.Trim() } | Select-Object -Last 18) -join "`n"
    $summary = $summary -replace '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}', '[account]'
    throw "TUI timed out waiting for '$Pattern'. Last terminal text: $summary"
}
function Get-TerminalText {
    $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\x1B\][^\x07]*(?:\x07)', ''
}
try {
    $codexDirectory = Join-Path $temporaryRoot 'codex home'
    $binDirectory = Join-Path $codexDirectory 'harness/bin'
    $workspace = Join-Path $temporaryRoot 'neutral workspace'
    New-Item -ItemType Directory -Path $binDirectory, $workspace -Force | Out-Null
    $registrations = @(
        @{ Destination = (Join-Path $binDirectory 'codex.ps1'); Source = (Join-Path $repository 'tools/codex.ps1') },
        @{ Destination = (Join-Path $codexDirectory 'AGENTS.md'); Source = (Join-Path $repository 'global/principles-of-work.md') }
    )
    if (Test-Path -LiteralPath (Join-Path $hostCodexHome 'auth.json')) {
        $registrations += @{ Destination = (Join-Path $codexDirectory 'auth.json'); Source = (Join-Path $hostCodexHome 'auth.json') }
    }
    foreach ($registration in $registrations) {
        New-Item -ItemType SymbolicLink -Path $registration.Destination -Target $registration.Source | Out-Null
        $links += $registration.Destination
    }
    @{ schemaVersion = 1; sourceRoot = $repository; codexCommand = $CodexCommand; profileName = 'harness'; configBridge = $ConfigBridge } |
        ConvertTo-Json | Set-Content -LiteralPath (Join-Path $codexDirectory 'harness/installation.json') -Encoding utf8
    $workspaceToml = ConvertTo-Json -InputObject $workspace -Compress
    "model = `"gpt-6-astra`"`nmodel_reasoning_effort = `"low`"`n[projects.$workspaceToml]`ntrust_level = `"trusted`"`n" |
        Set-Content -LiteralPath (Join-Path $codexDirectory 'config.toml') -Encoding utf8
    $entry = Join-Path $temporaryRoot 'entry.ps1'
    @'
Write-Output ('HARNESS_ENTRY=' + (Get-Command codex).Source)
codex --no-alt-screen
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $entry -Encoding utf8
    $env:CODEX_HOME = $codexDirectory
    $env:PATH = $binDirectory + [IO.Path]::PathSeparator + $previousPath
    $terminal = [Harness.Tests.ConPty]::new($pwsh, ('"' + $pwsh + '" -NoLogo -NoProfile -File "' + $entry + '"'), $workspace)
    Wait-Terminal 'OpenAI Codex|Welcome to Codex|Sign in' 30
    $transcript = $terminal.Transcript
    if (-not $transcript.Contains('HARNESS_ENTRY=' + (Join-Path $binDirectory 'codex.ps1'))) { throw 'Fresh terminal did not resolve the linked ordinary codex command.' }
    Write-Output 'Headless ConPTY reached the actual Codex TUI through ordinary codex resolution.'
    # This first stage intentionally gives an actionable signal before later
    # interaction so terminal support can be distinguished from TUI onboarding.
    Wait-Terminal 'gpt-6-astra low' 20
    Write-Output 'Actual TUI observes the local reasoning preference with live shared policy.'
    $terminal.Send('/status')
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    Wait-Terminal 'Context window|Approval|Full Access|Session ID|Session:' 20
    Write-Output 'Actual TUI /status responded.'
    $terminal.Send('/quit')
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    if (-not $terminal.Wait(10000)) {
        $terminal.Transcript | Set-Content -LiteralPath (Join-Path $temporaryRoot 'failure-transcript.txt') -Encoding utf8
        throw "Actual Codex TUI did not terminate after /quit. Diagnostic: $temporaryRoot/failure-transcript.txt"
    }
    if ($terminal.ExitCode -ne 0) { throw "Actual TUI /quit exited with $($terminal.ExitCode)." }
    Write-Output 'Actual TUI /quit completed with exit 0; no model request submitted.'
    $terminal.Dispose()
    $terminal = $null

    # Native /model must persist locally and survive a subsequent real launch.
    $localConfig = Join-Path $codexDirectory 'config.toml'
    (Get-Content -LiteralPath $localConfig -Raw).Replace('model_reasoning_effort = "low"','model_reasoning_effort = "xhigh"') | Set-Content -LiteralPath $localConfig -Encoding utf8
    $baseBeforeWriter = Get-Content -LiteralPath (Join-Path $codexDirectory 'config.toml') -Raw
    $terminal = [Harness.Tests.ConPty]::new($pwsh, ('"' + $pwsh + '" -NoLogo -NoProfile -File "' + $entry + '"'), $workspace)
    Wait-Terminal 'gpt-6-astra xhigh' 30
    $terminal.Send('/model')
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    Wait-Terminal 'Select model|Select Model|Choose model' 15
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    Wait-Terminal 'Select reasoning|reasoning effort|Reasoning Effort' 15
    Start-Sleep -Milliseconds 300
    $terminal.Send("`e[A")
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    while ((Get-Content -LiteralPath $localConfig -Raw) -notmatch 'model_reasoning_effort\s*=\s*"high"' -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if ((Get-Content -LiteralPath $localConfig -Raw) -notmatch 'model_reasoning_effort\s*=\s*"high"') {
        $terminal.Transcript | Set-Content -LiteralPath (Join-Path $temporaryRoot 'failure-transcript.txt') -Encoding utf8
        throw "Native /model writer did not persist the selected effort locally. Diagnostic: $temporaryRoot/failure-transcript.txt"
    }
    if ((Get-FileHash -LiteralPath $sharedProfile).Hash -cne $sharedProfileHash) { throw 'Native /model modified shared source.' }
    Write-Output 'Native /model persisted xhigh -> high locally; shared source stayed unchanged.'
    $terminal.Send('/quit')
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    if (-not $terminal.Wait(15000) -or $terminal.ExitCode -ne 0) { throw 'Writer fixture TUI did not quit cleanly.' }
    $terminal.Dispose()
    $terminal = $null

    # Exercise native trust persistence in a fresh owned repository.
    $trustWorkspace = Join-Path $temporaryRoot 'untrusted git workspace'
    New-Item -ItemType Directory -Path $trustWorkspace | Out-Null
    & git -C $trustWorkspace init --quiet
    if ($LASTEXITCODE -ne 0) { throw 'Could not create the disposable trust repository.' }
    $baseBeforeTrust = Get-Content -LiteralPath (Join-Path $codexDirectory 'config.toml') -Raw
    $trustEntry = Join-Path $temporaryRoot 'trust-entry.ps1'
    @'
# Select the supported token-based mechanism so this read-only UI fixture
# cannot trigger elevated Windows sandbox/account provisioning.
codex --no-alt-screen -c 'windows.sandbox="unelevated"'
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $trustEntry -Encoding utf8
    $terminal = [Harness.Tests.ConPty]::new($pwsh, ('"' + $pwsh + '" -NoLogo -NoProfile -File "' + $trustEntry + '"'), $trustWorkspace)
    Wait-Terminal 'Do\s*you\s*trust\s*the\s*contents|gpt-6-astra high' 30
    $trustPromptShown = (Get-TerminalText) -match 'Do\s*you\s*trust\s*the\s*contents'
    if ($trustPromptShown) {
        if ((Get-TerminalText) -match 'continue\s*and\s*create\s*a\s*sandbox') { throw 'Trust fixture would create a Windows sandbox; no global setup is authorized by this test.' }
        Start-Sleep -Milliseconds 300
        $terminal.Send("`r")
        Wait-Terminal 'gpt-6-astra high' 30
        $baseAfterTrust = Get-Content -LiteralPath (Join-Path $codexDirectory 'config.toml') -Raw
        $baseChanged = $baseAfterTrust -cne $baseBeforeTrust
        if (-not $baseChanged -or -not ($baseAfterTrust.Contains('untrusted git workspace') -and $baseAfterTrust -match 'trust_level\s*=\s*"trusted"')) { throw 'The accepted trust decision was not persisted locally.' }
        if ((Get-FileHash -LiteralPath $sharedProfile).Hash -cne $sharedProfileHash) { throw 'Native trust writer modified shared source.' }
        Write-Output 'Native trust decision persisted in local base configuration; shared source unchanged.'
    } else {
        throw 'Trust fixture did not exercise the native trust writer.'
    }
    $terminal.Send('/quit')
    Start-Sleep -Milliseconds 300
    $terminal.Send("`r")
    if (-not $terminal.Wait(15000) -or $terminal.ExitCode -ne 0) { throw 'Trust fixture TUI did not quit cleanly.' }
    if ((Get-FileHash -LiteralPath $sharedProfile -Algorithm SHA256).Hash -cne $sharedProfileHash) { throw 'The real repository shared profile changed during TUI tests.' }
    $completed = $true
} finally {
    if ($terminal) { $terminal.Dispose() }
    $env:CODEX_HOME = $previousCodexHome
    $env:PATH = $previousPath
    $resolvedTemp = [IO.Path]::GetFullPath($temporaryRoot)
    $tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTemp.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolvedTemp) -notlike 'codex-tui проверка *') { throw "Unsafe fixture cleanup: $resolvedTemp" }
    foreach ($link in $links) { if (Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue) { Remove-Item -LiteralPath $link -Force } }
    if (Test-Path -LiteralPath $resolvedTemp) {
        if ($KeepFailureArtifact -and -not $completed) { Write-Output "Failure fixture retained without active links: $resolvedTemp" }
        else { Remove-Item -LiteralPath $resolvedTemp -Recurse -Force }
    }
}
