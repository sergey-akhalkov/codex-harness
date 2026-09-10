#requires -Version 7.4
# The dispatch table follows `codex --help` from the supported CLI (0.153.4).
# Dispatch preserves explicit overrides; Rust supplies live shared defaults.
Set-StrictMode -Version Latest
$script:SingleValueOptions = @('-c', '--config', '--enable', '--disable', '--remote',
    '--remote-auth-token-env', '-m', '--model', '--local-provider', '-p',
    '--profile', '-s', '--sandbox', '-C', '--cd', '--add-dir', '-a',
    '--ask-for-approval', '--thread-source', '--output-schema', '--color',
    '-o', '--output-last-message', '--base', '--commit', '--title')

function Get-HarnessTaskArguments {
    [CmdletBinding()]
    param([AllowEmptyCollection()][AllowEmptyString()][string[]]$Arguments = @())
    # An optional first-position selector only; never scan prompt text as code.
    if (-not $Arguments.Count -or $Arguments[0] -cnotmatch '^--harness-effort(?:=|$)') { return ,$Arguments }
    $offset = 1
    if ($Arguments[0] -cmatch '^--harness-effort=(.*)$') { $choice = $Matches[1] }
    else {
        if ($Arguments.Count -lt 2) { throw '--harness-effort requires routine, standard or demanding.' }
        $choice = $Arguments[1]; $offset = 2
    }
    $efforts = @{routine='low';standard='high';demanding='xhigh'}
    if (-not $efforts.ContainsKey($choice)) { throw 'Unknown task effort; choose routine, standard or demanding.' }
    [string[]]$rest = if ($Arguments.Count -gt $offset) { $Arguments[$offset..($Arguments.Count-1)] } else { @() }
    $explicit = $false
    for ($i=0; $i -lt $rest.Count; $i++) {
        $item = $rest[$i]
        if ($item -ceq '--') { break }
        if ($item -cmatch '^(?:--profile(?:=|$)|-p|--remote(?:=|$))') { $explicit=$true; break }
        if ($item -cin @('-c','--config') -and $i+1 -lt $rest.Count) {
            if ($rest[$i+1] -match '^\s*model_reasoning_effort\s*=') { $explicit=$true; break }
            $i++; continue
        }
        if ($item -cmatch '^(?:--config=|-c)\s*model_reasoning_effort\s*=') { $explicit=$true; break }
        if ($item -cin $script:SingleValueOptions) { $i++; continue }
        if ($item -cin @('-i','--image')) { while($i+1 -lt $rest.Count -and -not $rest[$i+1].StartsWith('-')) { $i++ } }
    }
    if ($explicit) { return ,$rest }
    return ,([string[]](@('-c',('model_reasoning_effort="' + $efforts[$choice] + '"')) + $rest))
}

function Get-HarnessArguments {
    [CmdletBinding()]
    param(
        [AllowEmptyCollection()][AllowEmptyString()][string[]] $Arguments = @(),
        [string] $ProfileName = 'harness'
    )

    $commands = @('agents', 'exec', 'e', 'review', 'login', 'logout', 'mcp', 'plugin',
        'mcp-server', 'app-server', 'remote-control', 'app', 'completion', 'update',
        'doctor', 'sandbox', 'debug', 'apply', 'a', 'resume', 'queue', 'archive',
        'delete', 'migrate-rollouts', 'unarchive', 'fork', 'cloud', 'exec-server',
        'features', 'help')
    $command = $null
    $debugCommand = $null
    $sawPositional = $false
    $commandPositionals = 0
    $preserve = $false

    for ($index = 0; $index -lt $Arguments.Count; $index++) {
        $argument = $Arguments[$index]
        if ($argument -ceq '--') { break }
        if ($argument -cin @('-h', '--help', '-V', '--version') -or $argument -cmatch '^-[hV]+$') { $preserve = $true; continue }
        if ($argument -cmatch '^--profile(?:=|$)' -or $argument -cmatch '^-p') {
            $preserve = $true
        }
        if ($argument -cmatch '^--remote(?:=|$)') { $preserve = $true }
        if ($argument -cin $script:SingleValueOptions) { $index++; continue }
        # Images are a variable-length option: words after --image are image
        # values until the next option, including words that name subcommands.
        if ($argument -cin @('-i', '--image') -or $argument -cmatch '^--image=' -or $argument -cmatch '^-i.') {
            while ($index + 1 -lt $Arguments.Count -and -not $Arguments[$index + 1].StartsWith('-')) { $index++ }
            continue
        }
        if ($argument.StartsWith('-') -and $argument -cne '-') { continue }
        if (-not $sawPositional) {
            $sawPositional = $true
            if ($argument -cin $commands) { $command = $argument }
        } else {
            $commandPositionals++
            if ($command -cin @('exec', 'e') -and $commandPositionals -eq 1 -and $argument -ceq 'help') { $preserve = $true }
            if ($command -ceq 'debug' -and $commandPositionals -eq 1) { $debugCommand = $argument }
        }
    }

    $session = $null -eq $command -or $command -cin @('exec', 'e', 'review', 'resume', 'fork')
    if ($command -ceq 'debug' -and $debugCommand -ceq 'prompt-input') { $session = $true }
    if ($session -and -not $preserve) { '--profile'; $ProfileName }
    # Do not stringify/reparse a command line: each original argument stays one
    # argument, including empty strings, embedded quotes and `--` prompt text.
    foreach ($argument in $Arguments) { $argument }
}

function Get-HarnessAdditionalRoots {
    [CmdletBinding()]
    param([AllowEmptyCollection()][AllowEmptyString()][string[]]$Arguments = @(),
        [string]$WorkingDirectory = (Get-Location).ProviderPath)
    $directory = $WorkingDirectory
    $roots = [Collections.Generic.List[string]]::new()
    for ($index = 0; $index -lt $Arguments.Count; $index++) {
        $argument = $Arguments[$index]
        if ($argument -ceq '--') { break }
        if ($argument -cmatch '^--remote(?:=|$)') { return }
        if ($argument -cin @('--add-dir','-C','--cd')) {
            if ($index + 1 -ge $Arguments.Count) { return }
            $value = $Arguments[++$index]
            if ($argument -ceq '--add-dir') { $roots.Add($value) } else { $directory = $value }
        } elseif ($argument -cmatch '^--add-dir=(.*)$') { $roots.Add($Matches[1])
        } elseif ($argument -cmatch '^--cd=(.*)$' -or $argument -cmatch '^-C(.+)$') { $directory = $Matches[1]
        } elseif ($argument -cin $script:SingleValueOptions) { $index++
        } elseif ($argument -cin @('-i','--image') -or $argument -cmatch '^--image=' -or $argument -cmatch '^-i.') {
            while ($index + 1 -lt $Arguments.Count -and -not $Arguments[$index + 1].StartsWith('-')) { $index++ }
        }
    }
    # Native 0.153.4 resolves --add-dir relative to the effective --cd directory.
    # Capture only explicit CLI roots; do not infer paths from prompts or shell text.
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    try {
        $directory = [IO.Path]::GetFullPath($directory, $WorkingDirectory)
        foreach ($root in $roots) {
            $full = [IO.Path]::GetFullPath($root, $directory)
            if ((Test-Path -LiteralPath $full -PathType Container) -and $seen.Add($full)) { $full }
        }
    } catch { return } # Preserve native CLI argument validation and exit behavior.
}

# Transitional shell transport only; TOML and precedence belong to Rust.
function Get-HarnessLaunchArguments($Registration, [string[]]$Arguments) {
    [string[]]$classified = @(Get-HarnessArguments -Arguments $Arguments -ProfileName $Registration.profileName)
    if ($classified.Count -eq $Arguments.Count) { return ,$Arguments }
    try {
        if (-not $Registration.PSObject.Properties['configBridge']) {
            throw 'Shared configuration bridge is missing.'
        }
        $codexDirectory = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
        $start = [Diagnostics.ProcessStartInfo]::new($Registration.configBridge)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardInput = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        foreach ($argument in @('config-overrides', '--source', $Registration.sourceRoot, '--codex-home', $codexDirectory)) { $start.ArgumentList.Add($argument) }
        $process = [Diagnostics.Process]::Start($start)
        try {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            $process.StandardInput.Close()
            if (-not $process.WaitForExit(5000)) {
                $process.Kill($true)
                $null = $process.WaitForExit(2000)
                throw 'Shared configuration bridge timed out.'
            }
            if ($process.ExitCode -ne 0 -or -not $stdout.Wait(1000) -or -not $stderr.Wait(1000)) { throw 'Shared configuration bridge is unavailable.' }
            $json = $stdout.GetAwaiter().GetResult()
            $decoded = ConvertFrom-Json -InputObject $json -NoEnumerate -ErrorAction Stop
            if ($decoded -isnot [array] -or @($decoded | Where-Object { $_ -isnot [string] }).Count) { throw 'Shared configuration bridge returned invalid arguments.' }
            [string[]]$defaults = $decoded
        } finally { $process.Dispose() }
        return ,([string[]]($defaults + $Arguments))
    } catch {
        [Console]::Error.WriteLine('codex-harness: Shared defaults unavailable; starting ordinary Codex CLI with your original arguments and local settings.')
        return ,$Arguments
    }
}

function Resolve-HarnessFile {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string] $Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    if ($item.PSIsContainer) { throw "Expected a file: $Path" }
    if ($item.LinkType) {
        $target = $item.ResolveLinkTarget($true)
        if ($null -eq $target -or -not $target.Exists) { throw "Link source is unavailable: $Path" }
        return $target.FullName
    }
    return $item.FullName
}

function Get-HarnessLaunchConfiguration {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string] $LauncherSource)
    $codexDirectory = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path ([Environment]::GetFolderPath('UserProfile')) '.codex' }
    $metadataPath = Join-Path $codexDirectory 'harness/installation.json'
    if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
        throw "Harness registration is missing at '$metadataPath'. Run install.ps1 from the checkout to connect this CODEX_HOME."
    }
    $metadata = Get-Content -LiteralPath $metadataPath -Raw -ErrorAction Stop | ConvertFrom-Json -ErrorAction Stop
    if ($metadata.schemaVersion -ne 1 -or $metadata.profileName -cne 'harness') {
        throw "Unsupported harness registration at '$metadataPath'. Run install.ps1 from the checkout to repair it."
    }
    if (-not [IO.Path]::IsPathFullyQualified($metadata.sourceRoot) -or -not [IO.Path]::IsPathFullyQualified($metadata.codexCommand)) {
        throw "Harness sourceRoot and codexCommand must be absolute paths in '$metadataPath'."
    }
    $registeredLauncher = if ($metadata.PSObject.Properties['launcherSource']) {
        Resolve-HarnessFile $metadata.launcherSource
    } else { Resolve-HarnessFile (Join-Path $metadata.sourceRoot 'tools/codex.ps1') }
    if (-not [string]::Equals($registeredLauncher, $LauncherSource, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Harness source registration does not match this launcher. Run install.ps1 from the intended checkout to reconnect it."
    }
    $originalSource = Resolve-HarnessFile $metadata.codexCommand
    if ([string]::Equals($originalSource, $LauncherSource, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Harness codexCommand points back to its launcher. Run install.ps1 to repair the original CLI registration."
    }
    if ([IO.Path]::GetExtension($metadata.codexCommand) -notin @('.ps1', '.exe')) {
        throw 'The original Codex command must be a PowerShell script or executable, not a cmd/bat shim.'
    }
    return $metadata
}

Export-ModuleMember -Function Get-HarnessArguments, Get-HarnessTaskArguments, Get-HarnessAdditionalRoots, Get-HarnessLaunchArguments, Resolve-HarnessFile, Get-HarnessLaunchConfiguration
