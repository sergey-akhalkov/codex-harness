#requires -Version 7.4
# Standalone tests: pwsh -NoProfile -File tests/launcher.Tests.ps1
# No Pester dependency; the native recorder uses the installed Node prerequisite.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = Split-Path $PSScriptRoot -Parent
Import-Module (Join-Path $repository 'tools/launcher.psm1') -Force
$assertions = 0
function Assert-Arguments([string[]] $Actual, [string[]] $Expected, [string] $Name) {
    if ((ConvertTo-Json -InputObject @($Actual) -Compress) -cne (ConvertTo-Json -InputObject @($Expected) -Compress)) {
        throw "$Name expected $(ConvertTo-Json -InputObject @($Expected) -Compress), got $(ConvertTo-Json -InputObject @($Actual) -Compress)"
    }
    $script:assertions++
}
function Assert-True([bool] $Condition, [string] $Name) {
    if (-not $Condition) { throw $Name }
    $script:assertions++
}

$dispatchCases = @(
    @{ Name = 'TUI'; Arguments = @(); Select = $true },
    @{ Name = 'TUI prompt'; Arguments = @('hello there'); Select = $true },
    @{ Name = 'exec'; Arguments = @('exec', 'hello'); Select = $true },
    @{ Name = 'exec alias'; Arguments = @('e', 'hello'); Select = $true },
    @{ Name = 'exec help subcommand'; Arguments = @('exec', 'help', 'resume'); Select = $false },
    @{ Name = 'exec resume help prompt'; Arguments = @('exec', 'resume', '--last', 'help'); Select = $true },
    @{ Name = 'short help cluster'; Arguments = @('-hV'); Select = $false },
    @{ Name = 'review'; Arguments = @('review', '--uncommitted'); Select = $true },
    @{ Name = 'resume'; Arguments = @('resume', '--last'); Select = $true },
    @{ Name = 'fork'; Arguments = @('fork', '--last'); Select = $true },
    @{ Name = 'debug prompt'; Arguments = @('debug', 'prompt-input', 'hello'); Select = $true },
    @{ Name = 'config before command'; Arguments = @('-c', 'note="a b"', 'exec', 'hello'); Select = $true },
    @{ Name = 'config resembles profile'; Arguments = @('-c', '-profile', 'exec'); Select = $true },
    @{ Name = 'attached config'; Arguments = @('-cnote="a b"', 'exec'); Select = $true },
    @{ Name = 'config before management'; Arguments = @('--config', 'note="exec"', 'mcp', 'list'); Select = $false },
    @{ Name = 'config equals'; Arguments = @('--config=note="a b"', 'exec'); Select = $true },
    @{ Name = 'explicit profile'; Arguments = @('--profile', 'other', 'exec'); Select = $false },
    @{ Name = 'profile after command'; Arguments = @('exec', '-p', 'other'); Select = $false },
    @{ Name = 'attached profile'; Arguments = @('-pother', 'exec'); Select = $false },
    @{ Name = 'profile equals'; Arguments = @('exec', '--profile=other'); Select = $false },
    @{ Name = 'end of options'; Arguments = @('exec', '--', '--profile', 'literal'); Select = $true },
    @{ Name = 'TUI end of options'; Arguments = @('--', 'mcp'); Select = $true },
    @{ Name = 'end option help text'; Arguments = @('exec', '--', '--help'); Select = $true },
    @{ Name = 'images contain command word'; Arguments = @('--image', 'one.png', 'mcp'); Select = $true },
    @{ Name = 'image terminated before management'; Arguments = @('--image', 'one.png', '--config', 'x=1', 'mcp', 'list'); Select = $false },
    @{ Name = 'debug options'; Arguments = @('debug', '-c', 'note="models"', 'prompt-input'); Select = $true },
    @{ Name = 'debug models'; Arguments = @('debug', 'models'); Select = $false },
    @{ Name = 'debug app'; Arguments = @('debug', 'app-server'); Select = $false },
    @{ Name = 'remote TUI'; Arguments = @('--remote', 'ws://localhost:9999'); Select = $false },
    @{ Name = 'remote equals'; Arguments = @('--remote=ws://localhost:9999'); Select = $false }
)
foreach ($command in @('agents', 'login', 'logout', 'mcp', 'plugin', 'mcp-server', 'app-server',
    'remote-control', 'app', 'completion', 'update', 'doctor', 'sandbox', 'apply', 'a',
    'queue', 'archive', 'delete', 'migrate-rollouts', 'unarchive', 'cloud', 'exec-server', 'features', 'help')) {
    $dispatchCases += @{ Name = "management $command"; Arguments = @($command); Select = $false }
}
foreach ($flag in @('-h', '--help', '-V', '--version')) {
    $dispatchCases += @{ Name = "help $flag"; Arguments = @($flag); Select = $false }
    $dispatchCases += @{ Name = "exec help $flag"; Arguments = @('exec', $flag); Select = $false }
}
foreach ($case in $dispatchCases) {
    $expected = @()
    if ($case.Select) { $expected += @('--profile', 'harness') }
    $expected += $case.Arguments
    Assert-Arguments @(Get-HarnessArguments -Arguments $case.Arguments) $expected $case.Name
}

$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("codex-launcher проба " + [guid]::NewGuid().ToString('N'))
$pwsh = (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source
$node = (Get-Command node -CommandType Application | Select-Object -First 1).Source
$originalCodexHome = $env:CODEX_HOME
$originalNode = $env:HARNESS_RECORDER_NODE
try {
    New-Item -ItemType Directory -Path (Join-Path $temporaryRoot 'harness/bin') -Force | Out-Null
    $recorderScript = Join-Path $temporaryRoot 'original codex.ps1'
    @'
if ($MyInvocation.ExpectingInput) {
    $input | & $env:HARNESS_RECORDER_NODE (Join-Path $PSScriptRoot 'recorder.cjs') @args
} else {
    & $env:HARNESS_RECORDER_NODE (Join-Path $PSScriptRoot 'recorder.cjs') @args
}
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $recorderScript -Encoding utf8
    @'
const fs = require('node:fs');
if (process.env.HARNESS_RECORDER_WAIT) {
  fs.writeFileSync(process.env.HARNESS_RECORDER_WAIT, String(process.pid));
  setInterval(() => {}, 1000);
} else {
  const stdin = process.env.HARNESS_RECORDER_STDIN ? fs.readFileSync(0, 'utf8') : null;
  process.stdout.write(JSON.stringify({argv: process.argv.slice(2), stdin, roots: process.env.HARNESS_LSP_WORKSPACE_ROOTS || null}) + '\n');
  process.stderr.write('native stderr marker\n');
  process.exit(Number(process.env.HARNESS_RECORDER_EXIT || 0));
}
'@ | Set-Content -LiteralPath (Join-Path $temporaryRoot 'recorder.cjs') -Encoding utf8
    $metadataPath = Join-Path $temporaryRoot 'harness/installation.json'
    $metadata = @{ schemaVersion = 1; sourceRoot = $repository; codexCommand = $recorderScript; profileName = 'harness' }
    $metadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8
    $launcher = Join-Path $temporaryRoot 'harness/bin/codex.ps1'
    New-Item -ItemType SymbolicLink -Path $launcher -Target (Join-Path $repository 'tools/codex.ps1') | Out-Null
    $runner = Join-Path $temporaryRoot 'runner.ps1'
    @'
[Console]::InputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
[string[]] $forwarded = @(ConvertFrom-Json $env:HARNESS_RECORDER_ARGUMENTS)
& $env:HARNESS_RECORDER_ENTRY @forwarded
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $runner -Encoding utf8

    function Invoke-Recorder {
        param([string[]] $Arguments = @(), [AllowNull()][string] $Stdin = $null,
            [int] $ExitCode = 0, [string] $EntryPoint = $launcher)
        $start = [Diagnostics.ProcessStartInfo]::new($pwsh)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.RedirectStandardInput = $true
        $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false)
        $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
        $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
        # Splat a serialized argv in PowerShell, avoiding pwsh -File's own
        # parameter-value rewriting (e.g. --remote=ws:// becomes two tokens).
        foreach ($argument in @('-NoLogo', '-NoProfile', '-File', $runner)) { $start.ArgumentList.Add($argument) }
        $start.Environment['CODEX_HOME'] = $temporaryRoot
        $start.Environment['HARNESS_RECORDER_NODE'] = $node
        $start.Environment['HARNESS_RECORDER_EXIT'] = [string]$ExitCode
        $start.Environment['HARNESS_RECORDER_ENTRY'] = $EntryPoint
        $start.Environment['HARNESS_RECORDER_ARGUMENTS'] = ConvertTo-Json -InputObject @($Arguments) -Compress
        if ($null -ne $Stdin) { $start.Environment['HARNESS_RECORDER_STDIN'] = '1' }
        $process = [Diagnostics.Process]::new()
        $process.StartInfo = $start
        try {
            $null = $process.Start()
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            if ($null -ne $Stdin) { $process.StandardInput.Write($Stdin) }
            $process.StandardInput.Close()
            if (-not $process.WaitForExit(15000)) { $process.Kill($true); throw 'Recorder invocation timed out.' }
            return @{ ExitCode = $process.ExitCode; Stdout = $stdout.GetAwaiter().GetResult(); Stderr = $stderr.GetAwaiter().GetResult() }
        } finally { $process.Dispose() }
    }

    $primaryRoot = Join-Path $temporaryRoot 'primary'
    $additionalRoot = Join-Path $temporaryRoot 'additional root кириллица'
    New-Item -ItemType Directory -Path $primaryRoot,$additionalRoot | Out-Null
    $rootArgs = @('-C',$primaryRoot,'--add-dir','../additional root кириллица',('--add-dir='+$additionalRoot),'exec','hello')
    Assert-Arguments @(Get-HarnessAdditionalRoots -Arguments $rootArgs) @($additionalRoot) 'additional directories resolve from native cd and deduplicate'
    Assert-Arguments @(Get-HarnessAdditionalRoots -Arguments @('exec','--','--add-dir',$additionalRoot)) @() 'prompt text cannot expand diagnostic roots'
    Assert-Arguments @(Get-HarnessAdditionalRoots -Arguments @('-c','--add-dir','exec')) @() 'configuration option values cannot expand diagnostic roots'
    Assert-Arguments @(Get-HarnessAdditionalRoots -Arguments @('--remote=ws://localhost:9999','--add-dir',$additionalRoot)) @() 'remote CLI does not authorize local analysis roots'
    $rootResult = Invoke-Recorder -Arguments $rootArgs
    Assert-True ($rootResult.ExitCode -eq 0) 'additional-root native forwarding exits cleanly'
    Assert-Arguments @((ConvertFrom-Json $rootResult.Stdout).roots | ConvertFrom-Json) @($additionalRoot) 'actual native child inherits explicit diagnostic roots as JSON'

    $trickyArguments = @('exec', '-c', 'message="some spaces and quoted text"', '--', '',
        'путь с пробелами', 'quote"inside', 'C:\trailing slash\', '--profile', "line 1`nline 2")
    $result = Invoke-Recorder -Arguments $trickyArguments -ExitCode 37
    Assert-True ($result.ExitCode -eq 37) 'Native exit code must survive the launcher.'
    Assert-True ($result.Stderr -ceq "native stderr marker`n") 'Native stderr must stay on stderr.'
    Assert-Arguments @((ConvertFrom-Json $result.Stdout).argv) (@('--profile', 'harness') + $trickyArguments) 'Native argument boundaries'
    $nativeCases = $dispatchCases | Where-Object { $_.Name -notlike 'management *' -and $_.Name -notlike 'help *' -and $_.Name -notlike 'exec help *' }
    $nativeCases += $dispatchCases | Where-Object { $_.Name -in @('management mcp', 'management app-server', 'help --version', 'exec help --help', 'exec help subcommand') }
    foreach ($case in $nativeCases) {
        $result = Invoke-Recorder -Arguments $case.Arguments
        Assert-True ($result.ExitCode -eq 0) "Native dispatch exits cleanly: $($case.Name): $($result.Stderr)"
        $expected = @()
        if ($case.Select) { $expected += @('--profile', 'harness') }
        $expected += $case.Arguments
        Assert-Arguments @((ConvertFrom-Json $result.Stdout).argv) $expected "Native dispatch: $($case.Name)"
    }
    $stdinText = "first line`nвторая строка`n"
    $result = Invoke-Recorder -Arguments @('exec', '-') -Stdin $stdinText
    Assert-True ($result.ExitCode -eq 0) "Redirected stdin exits cleanly: $($result.Stderr)"
    Assert-True ((ConvertFrom-Json $result.Stdout).stdin -ceq $stdinText) "Redirected stdin must reach native CLI unchanged: $($result.Stdout)"

    $pipelineEntry = Join-Path $temporaryRoot 'pipeline.ps1'
    @'
$env:HARNESS_RECORDER_STDIN = '1'
@('pipeline first', 'pipeline вторая') | & (Join-Path $env:CODEX_HOME 'harness/bin/codex.ps1') exec -
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $pipelineEntry -Encoding utf8
    $result = Invoke-Recorder -EntryPoint $pipelineEntry
    Assert-True ($result.ExitCode -eq 0) "PowerShell pipeline exits cleanly: $($result.Stderr)"
    $pipelineText = 'pipeline first' + [Environment]::NewLine + 'pipeline вторая' + [Environment]::NewLine
    Assert-True ((ConvertFrom-Json $result.Stdout).stdin -ceq $pipelineText) "PowerShell pipeline input reaches native CLI: $($result.Stdout)"

    $metadata.codexCommand = $launcher
    $metadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8
    $result = Invoke-Recorder -Arguments @('--version')
    Assert-True ($result.ExitCode -eq 1 -and $result.Stderr.Contains('points back')) 'Direct link recursion must fail before invoking CLI.'
    $indirect = Join-Path $temporaryRoot 'indirect.ps1'
    '& (Join-Path $env:CODEX_HOME ''harness/bin/codex.ps1'') @args; exit $LASTEXITCODE' | Set-Content -LiteralPath $indirect -Encoding utf8
    $metadata.codexCommand = $indirect
    $metadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8
    $result = Invoke-Recorder -Arguments @('--version')
    Assert-True ($result.ExitCode -eq 1 -and $result.Stderr.Contains('Recursive')) 'Indirect same-process recursion must fail without looping.'

    $metadata.codexCommand = $recorderScript
    $metadata | ConvertTo-Json | Set-Content -LiteralPath $metadataPath -Encoding utf8
    $env:CODEX_HOME = $temporaryRoot
    $env:HARNESS_RECORDER_NODE = $node
    $waitFile = Join-Path $temporaryRoot 'waiting-native.pid'
    $env:HARNESS_RECORDER_WAIT = $waitFile
    $pipeline = [PowerShell]::Create()
    try {
        $null = $pipeline.AddCommand($launcher).AddArgument('exec').AddArgument('wait')
        $pending = $pipeline.BeginInvoke()
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        while (-not (Test-Path -LiteralPath $waitFile) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        Assert-True (Test-Path -LiteralPath $waitFile) 'Cancellation fixture must enter native execution.'
        $nativePid = [int](Get-Content -LiteralPath $waitFile)
        $pipeline.Stop()
        $nativeProcess = Get-Process -Id $nativePid -ErrorAction SilentlyContinue
        if ($nativeProcess) { $null = $nativeProcess.WaitForExit(5000) }
        Assert-True ($null -eq (Get-Process -Id $nativePid -ErrorAction SilentlyContinue)) 'Stopping the PowerShell pipeline terminates the native child.'
    } finally {
        $pipeline.Dispose()
        Remove-Item Env:HARNESS_RECORDER_WAIT -ErrorAction SilentlyContinue
    }

    Write-Output "Launcher checks passed ($assertions assertions; real native argv, streams, exit status, pipeline cancellation and recursion)."
} finally {
    $env:CODEX_HOME = $originalCodexHome
    $env:HARNESS_RECORDER_NODE = $originalNode
    # Delete only the fixture root just created, after checking its resolved
    # absolute identity and removing the launcher link without recursion.
    $resolvedTemp = [IO.Path]::GetFullPath($temporaryRoot)
    $tempParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedTemp.StartsWith($tempParent, [StringComparison]::OrdinalIgnoreCase) -or
        [IO.Path]::GetFileName($resolvedTemp) -notlike 'codex-launcher проба *') { throw "Unsafe fixture cleanup path: $resolvedTemp" }
    $fixtureLink = Join-Path $resolvedTemp 'harness/bin/codex.ps1'
    if (Test-Path -LiteralPath $fixtureLink) { Remove-Item -LiteralPath $fixtureLink -Force }
    if (Test-Path -LiteralPath $resolvedTemp) { Remove-Item -LiteralPath $resolvedTemp -Recurse -Force }
}
