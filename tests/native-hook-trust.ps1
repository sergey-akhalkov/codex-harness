# Headless native /hooks review for isolated probes. No hash writes or bypass.
# Callers own the reviewed CODEX_HOME and the exact hook definitions it contains.
function Invoke-NativeHookTrust {
    param(
        [Parameter(Mandatory)][string]$NativeCodex,
        [Parameter(Mandatory)][string]$CodexHome,
        [Parameter(Mandatory)][string]$Workspace,
        [Parameter(Mandatory)][string]$TracePath,
        [switch]$TrustWorkspace
    )
    if (-not ('Harness.Tests.ConPty' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'ConPty.cs') }
    . (Join-Path $PSScriptRoot 'consumer-rpc.ps1')
    $oldHome = $env:CODEX_HOME
    $terminal = $null
    $server = $null
    function Get-ReviewText {
        $terminal.Transcript -replace '\x1B\[[0-?]*[ -/]*[@-~]', '' -replace '\x1B\][^\x07]*(?:\x07)', ''
    }
    function Wait-ReviewText([string]$Pattern, [int]$Seconds = 20) {
        $deadline = [DateTime]::UtcNow.AddSeconds($Seconds)
        while ([DateTime]::UtcNow -lt $deadline) {
            if ((Get-ReviewText) -match $Pattern) { return }
            if ($terminal.HasExited) { throw "Native trust UI exited while waiting for $Pattern" }
            Start-Sleep -Milliseconds 100
        }
        throw "Native trust UI timed out waiting for $Pattern"
    }
    try {
        $env:CODEX_HOME = $CodexHome
        $terminal = [Harness.Tests.ConPty]::new($NativeCodex, ('"' + $NativeCodex + '" --no-alt-screen'), $Workspace)
        $env:CODEX_HOME = $oldHome
        Wait-ReviewText 'Do you trust the contents of this directory|Hooks need review|gpt-[0-9]|Welcome to Codex|Sign in' 30
        if ((Get-ReviewText).Contains('Do you trust the contents of this directory')) {
            if (-not $TrustWorkspace) { throw 'The owned fixture needs native directory trust; pass -TrustWorkspace only for a reviewed workspace.' }
            $terminal.Send("`r")
            Wait-ReviewText 'Hooks need review|gpt-[0-9]|Welcome to Codex|Sign in' 30
        }
        # The model banner can precede the startup review dialog. Let that
        # dialog settle before sending keys; its "Trust all" text is distinct
        # from the /hooks table and must not satisfy the table wait below.
        Start-Sleep -Milliseconds 1000
        if ((Get-ReviewText).Contains('Hooks need review')) {
            Wait-ReviewText '2\. Trust all and continue' 20
            $terminal.Send("$([char]27)[B")
            Start-Sleep -Milliseconds 200
            $terminal.Send("`r")
        } else {
            $terminal.Send('/hooks'); Start-Sleep -Milliseconds 250; $terminal.Send("`r")
            Wait-ReviewText 'Press enter to view hooks|PostToolUse|PreToolUse|SubagentStop' 20
            Start-Sleep -Milliseconds 800
            $terminal.Send('t')
            Start-Sleep -Milliseconds 800
            $terminal.Send([string][char]27)
        }
        Start-Sleep -Milliseconds 800
        Start-Sleep -Milliseconds 200
        $terminal.Send('/quit'); Start-Sleep -Milliseconds 200; $terminal.Send("`r")
        if (-not $terminal.Wait(30000) -or $terminal.ExitCode -ne 0) { throw 'Native trust UI did not exit successfully' }
        # The visual terminal is only the supported action route. Exact trust
        # evidence comes from a fresh native consumer, for every event/handler.
        $server = Start-ConsumerServer $NativeCodex $CodexHome $Workspace
        $listed = Invoke-ConsumerRpc $server 'hooks/list' @{cwds=@($Workspace)}
        $hooks = @($listed.data | ForEach-Object { $_.hooks })
        if ($hooks.Count -eq 0 -or @($hooks | Where-Object trustStatus -NE 'trusted').Count) {
            throw 'Native review did not establish trust for every isolated hook'
        }
        return @{ExitCode=$terminal.ExitCode;TrustedCount=$hooks.Count;Hooks=$hooks}
    }
    finally {
        $env:CODEX_HOME = $oldHome
        if ($null -ne $server) { Stop-ConsumerServer $server }
        if ($null -ne $terminal) {
            [IO.File]::WriteAllText($TracePath, $terminal.Transcript)
            $terminal.Dispose()
        }
    }
}
