#requires -Version 7.4
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-HarnessFullPath([string] $Path) {
    if (-not [IO.Path]::IsPathRooted($Path)) { throw "Expected an absolute path: $Path" }
    [IO.Path]::GetFullPath($Path).TrimEnd([IO.Path]::DirectorySeparatorChar)
}

function Test-HarnessSamePath([string] $Left, [string] $Right) {
    if (-not $Left -or -not $Right) { return $false }
    $Left = [Environment]::ExpandEnvironmentVariables($Left.Trim('"'))
    $Right = [Environment]::ExpandEnvironmentVariables($Right.Trim('"'))
    if (-not [IO.Path]::IsPathFullyQualified($Left) -or -not [IO.Path]::IsPathFullyQualified($Right)) { return $false }
    [string]::Equals((Get-HarnessFullPath $Left), (Get-HarnessFullPath $Right), [StringComparison]::OrdinalIgnoreCase)
}

function Assert-HarnessWithin([string] $Path, [string] $Root) {
    $full = Get-HarnessFullPath $Path
    $prefix = (Get-HarnessFullPath $Root) + [IO.Path]::DirectorySeparatorChar
    if (-not $full.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path is outside the expected root '${Root}': $Path"
    }
}

function Get-HarnessItem([string] $Path) {
    Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
}

function Get-HarnessLinkTarget([string] $Path) {
    $item = Get-HarnessItem $Path
    if ($null -eq $item -or $item.LinkType -ne 'SymbolicLink') { return $null }
    $target = [string]$item.Target
    if (-not [IO.Path]::IsPathRooted($target)) { $target = Join-Path (Split-Path $Path) $target }
    Get-HarnessFullPath $target
}

function Assert-HarnessOrdinaryParents([string] $Path) {
    $parent = Split-Path (Get-HarnessFullPath $Path)
    while ($parent) {
        $item = Get-HarnessItem $parent
        if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "Parent directory is a reparse point; resolve this path explicitly first: $parent"
        }
        $next = Split-Path $parent
        if ($next -eq $parent) { break }
        $parent = $next
    }
}

function Remove-HarnessLink([string] $Path, [string] $ExpectedSource) {
    Assert-HarnessOrdinaryParents $Path
    if (-not (Test-HarnessSamePath (Get-HarnessLinkTarget $Path) $ExpectedSource)) {
        throw "Connection ownership changed; preserving destination: $Path"
    }
    # Removing a link itself never traverses the referenced source directory.
    Remove-Item -LiteralPath $Path -Force -ErrorAction Stop
}

function Write-HarnessJson([string] $Path, $Value) {
    Write-HarnessBytes $Path ([Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 20)))
}

function Write-HarnessBytes([string] $Path, [byte[]] $Bytes) {
    if ($null -eq $Bytes) { $Bytes = [byte[]]@() }
    Assert-HarnessOrdinaryParents $Path
    $existing = Get-HarnessItem $Path
    if ($existing -and ($existing.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Refusing to write metadata through a link: $Path"
    }
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($Bytes); $stream.Flush($true) } finally { $stream.Dispose() }
        [IO.File]::Move($temporary, $Path, $true)
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
}

function Read-HarnessJson([string] $Path) {
    $item = Get-HarnessItem $Path
    if (-not $item) { return $null }
    if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw "Expected an ordinary metadata file: $Path"
    }
    Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json -AsHashtable
}

function Get-HarnessPathValue([string] $Scope) {
    [Environment]::GetEnvironmentVariable('Path', [EnvironmentVariableTarget]$Scope)
}

function Set-HarnessPathValue([string] $Scope, [AllowNull()][string] $Value) {
    [Environment]::SetEnvironmentVariable('Path', $Value, [EnvironmentVariableTarget]$Scope)
}

function Get-HarnessPathWithout([string] $Value, [string] $Entry) {
    (@($Value -split ';' | Where-Object {
        if (-not $_) { return $true }
        -not [string]::Equals($_.TrimEnd('\'), $Entry.TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    })) -join ';'
}

function Get-HarnessInventory([string] $SourceRoot, [string] $CodexHome, [string] $UserHome, [bool] $IncludeCodeTools = $false) {
    $manifestPath = Join-Path $SourceRoot 'global/kit.psd1'
    $manifest = Import-PowerShellDataFile -LiteralPath $manifestPath
    if ($manifest.SchemaVersion -ne 1) { throw 'Unsupported kit inventory schema.' }
    foreach ($relative in @($manifest.RequiredFiles) + @($manifest.Profile, $manifest.Instructions)) {
        $source = Get-HarnessFullPath (Join-Path $SourceRoot $relative)
        Assert-HarnessWithin $source $SourceRoot
        Assert-HarnessOrdinaryParents $source
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing kit source: $source" }
        if ((Get-HarnessItem $source).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Kit source must belong to checkout: $source" }
        if ([IO.Path]::GetExtension($source) -in '.ps1', '.psm1') {
            $parseTokens = $null; $parseErrors = $null
            [void][Management.Automation.Language.Parser]::ParseFile($source, [ref]$parseTokens, [ref]$parseErrors)
            if ($parseErrors.Count) { throw "Invalid PowerShell source ${source}: $($parseErrors[0].Message)" }
        }
    }
    $links = [Collections.Generic.List[object]]::new()
    $links.Add(@{ destination = Join-Path $CodexHome 'AGENTS.md'; source = Join-Path $SourceRoot $manifest.Instructions; kind = 'instructions'; name = 'AGENTS' })
    $launcherHash = (Get-FileHash -LiteralPath (Join-Path $SourceRoot $manifest.Launcher) -Algorithm SHA256).Hash.ToLowerInvariant()
    $links.Add(@{ destination = Join-Path $CodexHome 'harness/bin/codex.ps1'; source = Join-Path $CodexHome "harness/launchers/$launcherHash/codex.ps1"; kind = 'launcher'; name = 'codex' })
    $links.Add(@{ destination = Join-Path $CodexHome 'harness/bin/codex-harness-check.ps1'; source = Join-Path $SourceRoot $manifest.DiagnosticLauncher; kind = 'diagnostic-launcher'; name = 'codex-harness-check' })
    $links.Add(@{ destination = Join-Path $CodexHome 'agents/codex-harness'; source = Join-Path $SourceRoot $manifest.Agents; kind = 'agents'; name = 'codex-harness' })
    if ($IncludeCodeTools) {
        $tokenSelection = Read-HarnessJson (Join-Path $CodexHome 'harness/token-workflow.json')
        $hookSource = if ($tokenSelection -and $tokenSelection.enabled) { $manifest.TokenHooks } else { $manifest.Hooks }
        $links.Add(@{ destination = Join-Path $CodexHome 'hooks.json'; source = Join-Path $SourceRoot $hookSource; kind = 'hooks'; name = 'code-tools' })
        $links.Add(@{ destination = Join-Path $CodexHome 'harness/bin/hook.ps1'; source = Join-Path $SourceRoot $manifest.HookLauncher; kind = 'hook-launcher'; name = 'code-tools-bootstrap' })
    }
    $agentNames = [Collections.Generic.List[object]]::new()
    foreach ($kind in 'skill', 'agent') {
        $root = Join-Path $SourceRoot $(if ($kind -eq 'skill') { $manifest.Skills } else { $manifest.Agents })
        if (-not (Test-Path -LiteralPath $root -PathType Container)) { throw "Missing source directory: $root" }
        Assert-HarnessWithin $root $SourceRoot
        Assert-HarnessOrdinaryParents (Join-Path $root 'probe')
        $children = if ($kind -eq 'skill') { @(Get-ChildItem -LiteralPath $root -Directory) } else { @(Get-ChildItem -LiteralPath $root -File -Recurse -Filter '*.toml') }
        $names = @{}
        foreach ($child in $children) {
            if ($child.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Source must live in the checkout: $($child.FullName)" }
            $descriptionFile = if ($kind -eq 'skill') { Join-Path $child.FullName 'SKILL.md' } else { $child.FullName }
            if (-not (Test-Path -LiteralPath $descriptionFile)) { throw "Missing source descriptor: $descriptionFile" }
            $body = Get-Content -LiteralPath $descriptionFile -Raw
            $pattern = if ($kind -eq 'skill') { '(?m)^name:\s*["'']?([^\r\n"'']+)["'']?\s*$' } else { '(?m)^name\s*=\s*["'']([^"'']+)["'']\s*$' }
            $match = [regex]::Match($body, $pattern)
            if (-not $match.Success) { throw "Missing simple unique name in: $descriptionFile" }
            $name = $match.Groups[1].Value.Trim()
            if ($kind -eq 'agent' -and $name -cnotmatch '^[a-z][a-z0-9_-]*$') { throw "Invalid portable agent name in $descriptionFile" }
            if ($names.ContainsKey($name)) { throw "Duplicate $kind name '$name' in checkout." }
            $names[$name] = $true
            $destinationRoot = if ($kind -eq 'skill') { Join-Path $UserHome '.agents/skills' } else { Join-Path $CodexHome 'agents' }
            if ($kind -eq 'skill') { $links.Add(@{ destination = Join-Path $destinationRoot $child.Name; source = $child.FullName; kind = $kind; name = $name }) }
            else { $agentNames.Add(@{ name = $name; source = $child.FullName }) }
        }
    }
    @{ manifest = $manifest; links = $links.ToArray(); agentNames = $agentNames.ToArray() }
}

function Assert-HarnessState($State, [string] $CodexHome, [string] $UserHome) {
    if (-not $State) { return }
    if ($State.schemaVersion -ne 1 -or -not (Test-HarnessSamePath $State.codexHome $CodexHome) -or -not (Test-HarnessSamePath $State.userHome $UserHome)) {
        throw 'Installation metadata does not match this host. Preserve it and inspect the recorded paths.'
    }
    foreach ($link in $State.links) {
        $allowed = (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'AGENTS.md')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'harness.config.toml')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'harness/bin/codex.ps1')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'harness/bin/codex-harness-check.ps1')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'agents/codex-harness')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'hooks.json')) -or
            (Test-HarnessSamePath $link.destination (Join-Path $CodexHome 'harness/bin/hook.ps1')) -or
            (Test-HarnessSamePath (Split-Path $link.destination) (Join-Path $UserHome '.agents/skills'))
        if (-not $allowed) { throw "Unexpected destination in installation metadata: $($link.destination)" }
        Assert-HarnessOrdinaryParents $link.destination
    }
}

function Assert-HarnessPrerequisites($Manifest, [string] $CodexCommand) {
    if (-not $IsWindows) { throw 'This installer currently supports Windows native PowerShell only.' }
    if ($PSVersionTable.PSVersion -lt [version]$Manifest.PowerShellMinimum) { throw "PowerShell $($Manifest.PowerShellMinimum)+ is required." }
    if (-not (Test-Path -LiteralPath $CodexCommand -PathType Leaf)) { throw "Original Codex command unavailable: $CodexCommand. Rerun with -CodexCommand pointing to its new installation." }
    $versionText = (& $CodexCommand --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $versionText -notmatch 'codex-cli (\d+\.\d+\.\d+)') { throw "Cannot verify Codex CLI: $versionText" }
    if ([version]$Matches[1] -lt [version]$Manifest.CodexMinimum) { throw "Codex $($Manifest.CodexMinimum)+ is required; observed $versionText." }
    $help = & $CodexCommand --help | Out-String
    if ($LASTEXITCODE -ne 0 -or $help -notmatch '<name>\.config\.toml' -or $help -notmatch '--profile') { throw 'This Codex CLI does not expose the required file-profile contract.' }
    $openspec = Get-Command openspec -ErrorAction SilentlyContinue
    if (-not $openspec) { throw 'OpenSpec CLI is required for the included skills; install the documented prerequisite first.' }
    $openVersion = (& $openspec.Source --version | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or $openVersion -notmatch '^(\d+\.\d+\.\d+)' -or [version]$Matches[1] -lt [version]$Manifest.OpenSpecMinimum) { throw "OpenSpec $($Manifest.OpenSpecMinimum)+ is required; observed $openVersion." }
    @{ codex = $versionText; openspec = $openVersion; powershell = $PSVersionTable.PSVersion.ToString() }
}

function New-HarnessDirectory([string] $Path, $Created) {
    if (Test-Path -LiteralPath $Path -PathType Container) { return }
    $parent = Split-Path $Path
    if ($parent -and -not (Test-Path -LiteralPath $parent)) { New-HarnessDirectory $parent $Created }
    Assert-HarnessOrdinaryParents $Path
    New-Item -ItemType Directory -Path $Path -ErrorAction Stop | Out-Null
    $Created.Add($Path)
}

function Undo-HarnessOperations($Operations) {
    $failures = [Collections.Generic.List[string]]::new()
    for ($i = $Operations.Count - 1; $i -ge 0; $i--) {
        $op = $Operations[$i]
        try {
            $current = Get-HarnessItem $op.destination
            if ($op.newSource -and $current -and (Test-HarnessSamePath (Get-HarnessLinkTarget $op.destination) $op.newSource)) {
                Remove-HarnessLink $op.destination $op.newSource
                $current = $null
            }
            if ($op.oldSource) {
                if (-not $current) { New-Item -ItemType SymbolicLink -Path $op.destination -Target $op.oldSource -ErrorAction Stop | Out-Null }
                elseif (-not (Test-HarnessSamePath (Get-HarnessLinkTarget $op.destination) $op.oldSource)) { throw "Cannot restore changed destination: $($op.destination)" }
            } elseif ($current) { throw "Cannot undo externally changed destination: $($op.destination)" }
        } catch { $failures.Add($_.Exception.Message) }
    }
    if ($failures.Count) { throw ($failures -join "`n") }
}

function Get-HarnessFreshPath([string] $Scope, [string] $Value) {
    if ($Scope -eq 'Process') { return $Value }
    [Environment]::ExpandEnvironmentVariables((Get-HarnessPathValue 'Machine') + ';' + $Value)
}

function Assert-HarnessCommandPrecedence([string] $EffectivePath, [string] $Bin) {
    foreach ($directory in $EffectivePath -split ';') {
        if (-not $directory) { continue }
        $directory = [Environment]::ExpandEnvironmentVariables($directory.Trim('"'))
        if (Test-HarnessSamePath $directory $Bin) { return }
        foreach ($extension in '.ps1', '.exe', '.cmd', '.bat', '.com') {
            if (Test-Path -LiteralPath (Join-Path $directory ('codex' + $extension)) -PathType Leaf) {
                throw "Command precedence conflict: $(Join-Path $directory ('codex' + $extension)) precedes $Bin. Resolve this earlier PATH entry before connecting."
            }
        }
    }
    throw "Command directory is absent from the effective PATH: $Bin"
}

function Test-HarnessRuntime($State) {
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.StandardOutputEncoding = [Text.UTF8Encoding]::new($false)
    $start.StandardErrorEncoding = [Text.UTF8Encoding]::new($false)
    $start.WorkingDirectory = Join-Path $State.codexHome 'harness'
    $start.Environment['CODEX_HOME'] = $State.codexHome
    $start.Environment['Path'] = Get-HarnessFreshPath $State.pathScope (Get-HarnessPathValue $State.pathScope)
    $start.ArgumentList.Add('-NoProfile')
    $start.ArgumentList.Add('-Command')
    $start.ArgumentList.Add('$ErrorActionPreference = "Stop"; [Console]::OutputEncoding = [Text.UTF8Encoding]::new($false); $entry = (Get-Command codex).Source; $payload = @(& codex debug prompt-input); if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }; @{ entry = $entry; prompt = @($payload | ConvertFrom-Json) } | ConvertTo-Json -Depth 100 -Compress')
    $process = [Diagnostics.Process]::Start($start)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(45000)) { $process.Kill($true); throw 'Codex startup check timed out after 45 seconds.' }
        $resultText = $stdout.GetAwaiter().GetResult()
        $errorText = $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne 0) { throw "Codex startup failed (exit $($process.ExitCode)): $errorText" }
        $result = $resultText | ConvertFrom-Json
        if (-not (Test-HarnessSamePath $result.entry (Join-Path $State.codexHome 'harness/bin/codex.ps1'))) { throw "Fresh PowerShell resolved a different codex command: '$($result.entry)' instead of '$(Join-Path $State.codexHome 'harness/bin/codex.ps1')'." }
        $texts = @($result.prompt | ForEach-Object { $_.content } | ForEach-Object { $_.text })
        $instructions = $State.links | Where-Object kind -eq 'instructions' | Select-Object -First 1
        $expected = (Get-Content -LiteralPath $instructions.source -Raw).Replace("`r`n", "`n").Trim()
        if (-not (($texts -join "`n").Replace("`r`n", "`n").Contains($expected))) { throw 'The complete global instruction source was not loaded.' }
        # Permissions are observed from runtime context, not inferred from file contents.
        $permissions = ($texts | Where-Object { $_ -match 'Filesystem sandboxing defines' }) -join "`n"
        if ($permissions -notmatch 'danger-full-access' -or $permissions -notmatch 'Approval policy is currently never') { throw 'Shared Full Access defaults did not resolve in the neutral startup check.' }
        return [pscustomobject]@{ entryPoint = $result.entry; fullGlobalInstructions = $true; approvalPolicy = 'never'; sandbox = 'danger-full-access'; directory = $start.WorkingDirectory }
    } finally { $process.Dispose() }
}

function Test-HarnessConnections($State) {
    if ($State.ContainsKey('launcherSource')) {
        $expectedLauncherHash = Split-Path (Split-Path $State.launcherSource) -Leaf
        if ($expectedLauncherHash -notmatch '^[a-f0-9]{64}$' -or
            (Get-FileHash -LiteralPath $State.launcherSource -Algorithm SHA256 -ErrorAction Stop).Hash -ine $expectedLauncherHash) {
            throw 'Installed command bootstrap changed; run explicit core update to recover its owned copy.'
        }
    }
    foreach ($link in $State.links) {
        if (-not (Test-HarnessSamePath (Get-HarnessLinkTarget $link.destination) $link.source)) { throw "Broken or changed connection: $($link.destination). Reconnect from the checkout after resolving ownership conflicts." }
        if (-not (Test-Path -LiteralPath $link.source)) { throw "Source unavailable: $($link.source). Run install.ps1 from its new location." }
    }
    if (-not (Test-Path -LiteralPath $State.codexCommand -PathType Leaf)) { throw 'Original Codex command moved. Reconnect using -CodexCommand.' }
    $entry = Join-Path $State.codexHome 'harness/bin'
    if (-not (@((Get-HarnessPathValue $State.pathScope) -split ';') | Where-Object { $_ -and (Test-HarnessSamePath $_ $entry) })) { throw "Command directory missing from $($State.pathScope) PATH: $entry" }
    Assert-HarnessCommandPrecedence (Get-HarnessFreshPath $State.pathScope (Get-HarnessPathValue $State.pathScope)) $entry
    $runtime = Test-HarnessRuntime $State
    [pscustomobject]@{ status = 'Connected'; sourceRoot = $State.sourceRoot; codexHome = $State.codexHome; launcher = Join-Path $entry 'codex.ps1'; links = $State.links.Count; skills = @($State.links | Where-Object kind -eq 'skill').Count; agents = @(Get-ChildItem -LiteralPath (Join-Path $State.sourceRoot 'global/agents') -Recurse -Filter '*.toml' -File).Count; versions = $State.versions; runtime = $runtime }
}

function Set-HarnessNativeFeature([string]$CodexHome, [string]$CodexCommand,
    [ValidateSet('hooks','code_mode')][string]$Feature, [bool]$Enabled) {
    $config = Join-Path $CodexHome 'config.toml'
    Assert-HarnessOrdinaryParents $config
    $item = Get-HarnessItem $config
    if ($item -and ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))) {
        throw 'Hook policy requires an ordinary base config; preserving the current path.'
    }
    $before = if ($item) { ,([IO.File]::ReadAllBytes($config)) } else { ,([byte[]]@()) }
    $root = Join-Path $CodexHome ('harness/hook-policy-' + [guid]::NewGuid().ToString('N'))
    Assert-HarnessWithin $root (Join-Path $CodexHome 'harness')
    [void][IO.Directory]::CreateDirectory($root)
    try {
        [IO.File]::WriteAllBytes((Join-Path $root 'config.toml'), $before)
        $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.WorkingDirectory = $root
        $start.Environment['CODEX_HOME'] = $root
        $start.Environment['HARNESS_POLICY_CODEX'] = $CodexCommand
        $start.Environment['HARNESS_POLICY_FEATURE'] = $Feature
        $start.Environment['HARNESS_POLICY_ACTION'] = if ($Enabled) { 'enable' } else { 'disable' }
        $start.Environment['HARNESS_POLICY_VALUE'] = $Enabled.ToString().ToLowerInvariant()
        foreach ($argument in @('-NoLogo','-NoProfile','-Command',
            '& $env:HARNESS_POLICY_CODEX features $env:HARNESS_POLICY_ACTION $env:HARNESS_POLICY_FEATURE *> $null; if ($LASTEXITCODE -ne 0) { exit 1 }; $result = & $env:HARNESS_POLICY_CODEX features list 2>$null; $pattern = "^" + [regex]::Escape($env:HARNESS_POLICY_FEATURE) + "\s+.+\s+" + $env:HARNESS_POLICY_VALUE + "\s*$"; if ($LASTEXITCODE -ne 0 -or -not ($result -match $pattern)) { exit 2 }')) {
            $start.ArgumentList.Add($argument)
        }
        $process = [Diagnostics.Process]::Start($start)
        try {
            $stdout = $process.StandardOutput.ReadToEndAsync()
            $stderr = $process.StandardError.ReadToEndAsync()
            if (-not $process.WaitForExit(30000)) { $process.Kill($true); throw 'Native hook-policy editor timed out; base config unchanged.' }
            $null = $stdout.GetAwaiter().GetResult()
            $null = $stderr.GetAwaiter().GetResult()
            if ($process.ExitCode -ne 0) { throw "Native hook-policy editor failed (exit $($process.ExitCode)); base config unchanged." }
        } finally { $process.Dispose() }
        $after = [IO.File]::ReadAllBytes((Join-Path $root 'config.toml'))
        if ([Convert]::ToBase64String($before) -ceq [Convert]::ToBase64String($after)) { return }
        $current = if (Test-Path -LiteralPath $config) { ,([IO.File]::ReadAllBytes($config)) } else { ,([byte[]]@()) }
        if ([Convert]::ToBase64String($before) -cne [Convert]::ToBase64String($current)) { throw 'Base config changed during hook-policy preparation; preserving concurrent edits.' }
        if ($item) {
            $backup = Join-Path $CodexHome ('harness/backups/feature-' + $Feature + '-' + [guid]::NewGuid().ToString('N') + '.toml')
            Assert-HarnessOrdinaryParents $backup
            [void][IO.Directory]::CreateDirectory((Split-Path $backup))
            Write-HarnessBytes $backup $before
        }
        Write-HarnessBytes $config $after
    } finally {
        Assert-HarnessWithin $root (Join-Path $CodexHome 'harness')
        Remove-Item -LiteralPath $root -Recurse -Force
    }
}

function Disable-HarnessHooks([string]$CodexHome, [string]$CodexCommand) {
    Set-HarnessNativeFeature $CodexHome $CodexCommand 'hooks' $false
}

function Get-HarnessNativeFeatures([string]$CodexHome, [string]$CodexCommand) {
    $start = [Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $start.Environment['CODEX_HOME']=$CodexHome
    $start.Environment['HARNESS_POLICY_CODEX']=$CodexCommand
    foreach($arg in @('-NoLogo','-NoProfile','-Command','& $env:HARNESS_POLICY_CODEX features list; exit $LASTEXITCODE')) { $start.ArgumentList.Add($arg) }
    $process = [Diagnostics.Process]::Start($start)
    try {
        $out=$process.StandardOutput.ReadToEndAsync(); $err=$process.StandardError.ReadToEndAsync()
        if(-not $process.WaitForExit(30000)) { $process.Kill($true); throw 'Native feature discovery timed out.' }
        $body=$out.GetAwaiter().GetResult(); $null=$err.GetAwaiter().GetResult()
        if($process.ExitCode) { throw 'Native feature discovery failed.' }
        $result=@{}
        foreach($line in $body -split '\r?\n') { if($line -match '^(hooks|code_mode)\s+.+\s+(true|false)\s*$') { $result[$Matches[1]]=$Matches[2] -eq 'true' } }
        if($result.Count -ne 2) { throw 'Native feature discovery returned incomplete coverage.' }
        return $result
    } finally { $process.Dispose() }
}

function Invoke-HarnessInstallCore {
    [CmdletBinding()]
    param([string] $SourceRoot, [string] $CodexHome, [string] $UserHome,
        [string] $CodexCommand, [string] $Mode = 'Install', [string] $PathScope = 'User', [switch] $Preview, [switch] $IncludeCodeTools, [switch] $DeferCommit, [string] $DependencyUserHome)
    $SourceRoot = Get-HarnessFullPath $SourceRoot
    $CodexHome = Get-HarnessFullPath $CodexHome
    $UserHome = Get-HarnessFullPath $UserHome
    $DependencyUserHome = Get-HarnessFullPath $(if ($DependencyUserHome) { $DependencyUserHome } else { $UserHome })
    if (Test-HarnessSamePath $SourceRoot $CodexHome) { throw 'The checkout cannot be CODEX_HOME.' }
    if ($CodexHome.StartsWith($SourceRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Keep Codex state outside the checkout.' }
    $stateRoot = Join-Path $CodexHome 'harness'
    $statePath = Join-Path $stateRoot 'installation.json'
    $pendingPath = Join-Path $stateRoot 'pending.json'
    Assert-HarnessOrdinaryParents $statePath
    $state = Read-HarnessJson $statePath
    Assert-HarnessState $state $CodexHome $UserHome
    if ($state -and $state.ContainsKey('dependencyUserHome') -and -not (Test-HarnessSamePath $state.dependencyUserHome $DependencyUserHome)) { throw 'Dependency owner differs from the recorded installation; preserve its connections.' }
    $pending = Read-HarnessJson $pendingPath
    if ($pending -and $Mode -ne 'Recover') { throw 'An interrupted installation needs recovery. Run install.ps1 -Mode Recover with the same user/home arguments.' }
    if ($Mode -eq 'Recover') {
        if (-not $pending) { return [pscustomobject]@{ status = 'No pending transaction' } }
        Assert-HarnessState $pending.plannedState $CodexHome $UserHome
        Assert-HarnessState $pending.previousState $CodexHome $UserHome
        foreach ($recorded in @($pending.plannedState,$pending.previousState)) {
            if ($recorded -and $recorded.ContainsKey('dependencyUserHome') -and -not (Test-HarnessSamePath $recorded.dependencyUserHome $DependencyUserHome)) { throw 'Dependency owner differs from the pending transaction; preserve it.' }
        }
        if ($pending.pathScope -notin 'User', 'Process') { throw 'Invalid transaction PATH scope.' }
        $recordedLinks = @($pending.plannedState.links) + $(if ($pending.previousState) { @($pending.previousState.links) } else { @() })
        foreach ($operation in $pending.operations) {
            $records = @($recordedLinks | Where-Object { Test-HarnessSamePath $_.destination $operation.destination })
            if (-not $records.Count) { throw "Unrecorded recovery destination: $($operation.destination)" }
            Assert-HarnessOrdinaryParents $operation.destination
            foreach ($source in @($operation.oldSource, $operation.newSource)) {
                if ($source -and -not @($records | Where-Object { Test-HarnessSamePath $_.source $source }).Count) { throw 'Recovery source is not owned by the recorded installation.' }
            }
        }
        if ($pending.ContainsKey('stateAfterHash')) {
            $currentBytes = if (Test-Path -LiteralPath $statePath) { ,([IO.File]::ReadAllBytes($statePath)) } else { ,([byte[]]@()) }
            $beforeBytes = if ($null -ne $pending.stateBeforeBytes) { ,([Convert]::FromBase64String($pending.stateBeforeBytes)) } else { ,([byte[]]@()) }
            $currentHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$currentBytes))
            if ($currentHash -notin @($pending.stateAfterHash, [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$beforeBytes)))) { throw 'Installation state changed after interruption; preserving concurrent changes.' }
        }
        if ($Preview) { return [pscustomobject]@{ status = 'Preview recovery'; operations = $pending.operations } }
        Undo-HarnessOperations $pending.operations
        if ((Get-HarnessPathValue $pending.pathScope) -ceq $pending.pathAfter) { Set-HarnessPathValue $pending.pathScope $pending.pathBefore }
        elseif ((Get-HarnessPathValue $pending.pathScope) -cne $pending.pathBefore) { throw 'PATH changed after interruption; preserving it. Resolve its harness entry before recovering again.' }
        if ($pending.ContainsKey('stateBeforeBytes') -and $null -ne $pending.stateBeforeBytes) { Write-HarnessBytes $statePath ([Convert]::FromBase64String($pending.stateBeforeBytes)) }
        elseif ($pending.previousState) { Write-HarnessJson $statePath $pending.previousState }
        elseif (Test-Path -LiteralPath $statePath) { Remove-Item -LiteralPath $statePath }
        Remove-Item -LiteralPath $pendingPath
        return [pscustomobject]@{ status = 'Recovered' }
    }
    if ($Mode -eq 'Check') {
        if (-not $state) { throw 'This Codex home has no kit installation.' }
        return Test-HarnessConnections $state
    }
    if ($Mode -eq 'Disconnect') {
        if (-not $state) { return [pscustomobject]@{ status = 'Not connected' } }
        $operations = @($state.links | Where-Object owned | ForEach-Object {
            $current = Get-HarnessItem $_.destination
            if ($current -and -not (Test-HarnessSamePath (Get-HarnessLinkTarget $_.destination) $_.source)) { throw "Ownership mismatch; preserving: $($_.destination)" }
            if ($current) { @{ destination = $_.destination; oldSource = $_.source; newSource = $null } }
        })
        $nextState = $null
        $scope = $state.pathScope
        $beforePath = Get-HarnessPathValue $scope
        $afterPath = if ($state.pathAdded) { Get-HarnessPathWithout $beforePath (Join-Path $CodexHome 'harness/bin') } else { $beforePath }
    } else {
        # A core update is additive with respect to already connected code tools.
        # Include their source links in reconciliation so they remain recorded,
        # follow checkout moves and can be repaired instead of becoming obsolete.
        $hasHookConnections = $state -and @($state.links | Where-Object { $_.kind -in 'hooks', 'hook-launcher' }).Count -gt 0
        $inventory = Get-HarnessInventory $SourceRoot $CodexHome $UserHome ($IncludeCodeTools -or $hasHookConnections)
        if (-not $CodexCommand) {
            $CodexCommand = if ($state) { $state.codexCommand } else { (Get-Command codex -ErrorAction Stop).Source }
        }
        $CodexCommand = Get-HarnessFullPath $CodexCommand
        if ((Test-HarnessSamePath $CodexCommand (Join-Path $CodexHome 'harness/bin/codex.ps1')) -or
            (Test-HarnessSamePath (Get-HarnessLinkTarget $CodexCommand) (Join-Path $SourceRoot 'tools/codex.ps1'))) { throw 'Original Codex command resolves to the kit launcher; use -CodexCommand with the real CLI.' }
        $versions = Assert-HarnessPrerequisites $inventory.manifest $CodexCommand
        if (Get-HarnessItem (Join-Path $CodexHome 'AGENTS.override.md')) { throw 'Global AGENTS.override.md would hide the kit instructions. Resolve composition before connecting.' }
        $oldLinks = @{}
        if ($state) { foreach ($link in $state.links) { $oldLinks[$link.destination] = $link } }
        $operations = [Collections.Generic.List[object]]::new()
        foreach ($link in $inventory.links) {
            Assert-HarnessOrdinaryParents $link.destination
            $current = Get-HarnessItem $link.destination
            $old = $oldLinks[$link.destination]
            $target = Get-HarnessLinkTarget $link.destination
            if ($current -and -not (Test-HarnessSamePath $target $link.source) -and -not ($old -and $old.owned -and (Test-HarnessSamePath $target $old.source))) { throw "Target conflict; preserving: $($link.destination)" }
            $link.owned = if ($old) { [bool]$old.owned } else { -not [bool]$current }
            if (-not $current) { $link.owned = $true }
            if (-not (Test-HarnessSamePath $target $link.source)) { $operations.Add(@{ destination = $link.destination; oldSource = $target; newSource = $link.source }) }
            $oldLinks.Remove($link.destination)
        }
        foreach ($old in $oldLinks.Values) {
            if (-not $old.owned) { continue }
            $current = Get-HarnessItem $old.destination
            if ($current -and -not (Test-HarnessSamePath (Get-HarnessLinkTarget $old.destination) $old.source)) { throw "Obsolete connection ownership changed; preserving: $($old.destination)" }
            if ($current) { $operations.Add(@{ destination = $old.destination; oldSource = $old.source; newSource = $null }) }
        }
        # Detect alternate filenames declaring the same global capability name.
        foreach ($link in @($inventory.links | Where-Object { $_.kind -in 'skill', 'agent' })) {
            $destinationParent = Split-Path $link.destination
            if (-not (Test-Path -LiteralPath $destinationParent)) { continue }
            foreach ($other in Get-ChildItem -LiteralPath $destinationParent -Force) {
                if (Test-HarnessSamePath $other.FullName $link.destination) { continue }
                $descriptor = if ($link.kind -eq 'skill') { Join-Path $other.FullName 'SKILL.md' } else { $other.FullName }
                if (-not (Test-Path -LiteralPath $descriptor -PathType Leaf)) { continue }
                $body = Get-Content -LiteralPath $descriptor -Raw
                $pattern = if ($link.kind -eq 'skill') { '(?m)^name:\s*["'']?' + [regex]::Escape($link.name) + '["'']?\s*$' } else { '(?m)^name\s*=\s*["'']' + [regex]::Escape($link.name) + '["'']\s*$' }
                if ($body -match $pattern) { throw "Capability name collision '$($link.name)' at $descriptor" }
            }
        }
        $globalAgents = Join-Path $CodexHome 'agents'
        $managedAgents = Join-Path $globalAgents 'codex-harness'
        $foreignAgentFiles = [Collections.Generic.List[string]]::new()
        if (Test-Path -LiteralPath $globalAgents) {
            foreach ($entry in Get-ChildItem -LiteralPath $globalAgents -Force) {
                if (Test-HarnessSamePath $entry.FullName $managedAgents) { continue }
                if ($entry.PSIsContainer) {
                    foreach ($file in Get-ChildItem -LiteralPath $entry.FullName -File -Filter '*.toml' -Recurse -FollowSymlink) { $foreignAgentFiles.Add($file.FullName) }
                } elseif ($entry.Extension -eq '.toml') { $foreignAgentFiles.Add($entry.FullName) }
            }
        }
        $baseConfig = Join-Path $CodexHome 'config.toml'
        $baseText = if (Test-Path -LiteralPath $baseConfig) { Get-Content -LiteralPath $baseConfig -Raw } else { '' }
        foreach ($agent in $inventory.agentNames) {
            $escapedName = [regex]::Escape($agent.name)
            foreach ($file in $foreignAgentFiles) {
                if ((Get-Content -LiteralPath $file -Raw) -match ('(?m)^\s*name\s*=\s*["'']' + $escapedName + '["'']\s*(?:#.*)?$')) {
                    throw "Agent name collision '$($agent.name)' at $file"
                }
            }
            $agentKey = '(?:' + $escapedName + '|"' + $escapedName + '"|''' + $escapedName + ''')'
            $agentsKey = '(?:agents|"agents"|''agents'')'
            $dottedDeclaration = '(?m)^\s*(?:\[\s*' + $agentsKey + '\s*\.\s*' + $agentKey + '\s*(?:\]|\.)|' + $agentsKey + '\s*\.\s*' + $agentKey + '\s*(?:\.|=))'
            $agentsSection = [regex]::Match($baseText, ('(?ms)^\s*\[\s*' + $agentsKey + '\s*\][^\r\n]*\r?\n(?<body>.*?)(?=^\s*\[|\z)')).Groups['body'].Value
            $sectionDeclaration = '(?m)^\s*' + $agentKey + '\s*(?:=|\.)'
            # Native TOML also accepts inline role tables, either inside the
            # agents section or inside a root agents = { ... } assignment.
            $inlineAgents = [regex]::Matches($baseText, ('(?m)^\s*' + $agentsKey + '\s*=\s*\{[^\r\n]*'))
            $inlineDeclaration = '[{,]\s*' + $agentKey + '\s*(?:=|\.)'
            $inlineCollision = @($inlineAgents | Where-Object { $_.Value -match $inlineDeclaration }).Count -gt 0
            if ($baseText -match $dottedDeclaration -or $agentsSection -match $sectionDeclaration -or $inlineCollision) {
                throw "Agent name collision '$($agent.name)' in local config.toml"
            }
        }
        $scope = if ($state) { $state.pathScope } else { $PathScope }
        $beforePath = Get-HarnessPathValue $scope
        $bin = Join-Path $CodexHome 'harness/bin'
        $pathExists = @($beforePath -split ';' | Where-Object { $_ -and (Test-HarnessSamePath $_ $bin) }).Count -gt 0
        $afterPath = if ($pathExists) { $beforePath } else { $bin + ';' + $beforePath }
        Assert-HarnessCommandPrecedence (Get-HarnessFreshPath $scope $afterPath) $bin
        $nextState = @{ schemaVersion = 1; sourceRoot = $SourceRoot; codexHome = $CodexHome; userHome = $UserHome;
            codexCommand = $CodexCommand; profileName = $inventory.manifest.ProfileName; links = @($inventory.links);
            pathScope = $scope; pathAdded = if ($state) { $state.pathAdded } else { -not $pathExists }; versions = $versions }
        if ($IncludeCodeTools -or ($state -and $state.ContainsKey('dependencyUserHome'))) { $nextState.dependencyUserHome = $DependencyUserHome }
        $operations = $operations.ToArray()
    }
    if ($Preview) { return [pscustomobject]@{ status = "Preview $Mode"; operations = $operations; pathScope = $scope; pathChange = ($beforePath -cne $afterPath); note = 'No files or environment values changed; actual link creation is checked during activation.' } }
    if ($nextState) {
        # The command entry must survive an unavailable checkout. Stage a
        # content-addressed bootstrap before the existing link journal commits.
        # Its shared policy module stays live through sourceRoot when healthy.
        $launcherLink = $nextState.links | Where-Object kind -EQ 'launcher' | Select-Object -First 1
        $launcherBytes = [IO.File]::ReadAllBytes((Join-Path $SourceRoot $inventory.manifest.Launcher))
        $launcherHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($launcherBytes)).ToLowerInvariant()
        $launcherPath = Join-Path $CodexHome "harness/launchers/$launcherHash/codex.ps1"
        if (-not (Test-HarnessSamePath $launcherLink.source $launcherPath)) { throw 'Launcher source changed during preparation; existing connections preserved.' }
        Assert-HarnessOrdinaryParents $launcherPath
        $existingLauncher = Get-HarnessItem $launcherPath
        if ($existingLauncher) {
            if ($existingLauncher.PSIsContainer -or $existingLauncher.Attributes -band [IO.FileAttributes]::ReparsePoint -or
                (Get-FileHash -LiteralPath $launcherPath -Algorithm SHA256).Hash -ine $launcherHash) { throw 'Owned launcher copy changed; preserving it for explicit recovery.' }
        } else {
            New-Item -ItemType Directory -Path (Split-Path $launcherPath) -Force | Out-Null
            Write-HarnessBytes $launcherPath $launcherBytes
        }
        $nextState.launcherSource = $launcherPath
        $buildState = Join-Path $CodexHome 'harness/config-bridge'
        if ($state -and $state.ContainsKey('configBridge') -and (Test-Path -LiteralPath $state.configBridge)) {
            $prepared = & $state.configBridge build --source $SourceRoot --state $buildState
        } else {
            $prepared = & cargo run --quiet --locked --jobs 1 --manifest-path (Join-Path $SourceRoot 'Cargo.toml') --target-dir (Join-Path $SourceRoot 'target') -p codex-harness --bin codex-harness -- build --source $SourceRoot --state $buildState
        }
        if ($LASTEXITCODE -ne 0) { throw 'Native configuration bridge build failed; existing connections preserved.' }
        $nextState.configBridge = Join-Path (($prepared | Out-String | ConvertFrom-Json).build) 'codex-harness.exe'
        & $nextState.configBridge config-localize --source $SourceRoot --codex-home $CodexHome
        if ($LASTEXITCODE -ne 0) { throw 'Legacy configuration migration failed; inspect local recovery copies before retrying.' }
    }
    $createdDirectories = [Collections.Generic.List[string]]::new()
    $applied = [Collections.Generic.List[object]]::new()
    $stateBeforeBytes = if (Test-Path -LiteralPath $statePath) { [Convert]::ToBase64String([IO.File]::ReadAllBytes($statePath)) } else { $null }
    $stateAfterBytes = if ($nextState) { ,([Text.UTF8Encoding]::new($false).GetBytes(($nextState | ConvertTo-Json -Depth 20))) } else { ,([byte[]]@()) }
    try {
        New-HarnessDirectory $stateRoot $createdDirectories
        $pendingRecord = @{ previousState = $state; plannedState = if ($nextState) { $nextState } else { $state }; operations = $operations; pathScope = $scope; pathBefore = $beforePath; pathAfter = $afterPath;
            stateBeforeBytes = $stateBeforeBytes; stateAfterHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$stateAfterBytes)) }
        Write-HarnessJson $pendingPath $pendingRecord
        foreach ($op in $operations) {
            New-HarnessDirectory (Split-Path $op.destination) $createdDirectories
            # Register the inverse before mutation, including a removed old link.
            $applied.Add($op)
            if ($op.oldSource) { Remove-HarnessLink $op.destination $op.oldSource }
            if ($op.newSource) {
                try { New-Item -ItemType SymbolicLink -Path $op.destination -Target $op.newSource -ErrorAction Stop | Out-Null }
                catch { throw "Cannot create direct link $($op.destination): $($_.Exception.Message). Check Windows Developer Mode/link privilege; no copy fallback is used." }
            }
        }
        $tokenSelection = Read-HarnessJson (Join-Path $CodexHome 'harness/token-workflow.json')
        # An accepted RTK installation owns its explicit selection. Preserve a
        # subsequent native suspension instead of re-enabling hooks on update.
        if ($nextState -and -not ($tokenSelection -and $tokenSelection.enabled)) { Disable-HarnessHooks $CodexHome $CodexCommand }
        Set-HarnessPathValue $scope $afterPath
        if ($nextState) { Write-HarnessBytes $statePath $stateAfterBytes }
        elseif (Test-Path -LiteralPath $statePath) { Remove-Item -LiteralPath $statePath }
        $verified = if ($nextState) { Test-HarnessConnections $nextState } else { $null }
        if (-not $DeferCommit) { Remove-Item -LiteralPath $pendingPath }
    } catch {
        $original = $_.Exception.Message
        try {
            $currentStateBytes = if (Test-Path -LiteralPath $statePath) { ,([IO.File]::ReadAllBytes($statePath)) } else { ,([byte[]]@()) }
            $priorStateBytes = if ($null -ne $stateBeforeBytes) { ,([Convert]::FromBase64String($stateBeforeBytes)) } else { ,([byte[]]@()) }
            $currentStateHash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$currentStateBytes))
            if ($currentStateHash -notin @([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$stateAfterBytes)), [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([byte[]]$priorStateBytes)))) { throw 'Installation state changed concurrently; preserving it for explicit recovery.' }
            Undo-HarnessOperations $applied
            if ((Get-HarnessPathValue $scope) -ceq $afterPath) { Set-HarnessPathValue $scope $beforePath }
            elseif ((Get-HarnessPathValue $scope) -cne $beforePath) { throw 'PATH changed concurrently; preserving the new value. Resolve it before -Mode Recover.' }
            if ($null -ne $stateBeforeBytes) { Write-HarnessBytes $statePath ([Convert]::FromBase64String($stateBeforeBytes)) }
            elseif (Test-Path -LiteralPath $statePath) { Remove-Item -LiteralPath $statePath }
            if (Test-Path -LiteralPath $pendingPath) { Remove-Item -LiteralPath $pendingPath }
            for ($i = $createdDirectories.Count - 1; $i -ge 0; $i--) {
                if (Test-Path -LiteralPath $createdDirectories[$i]) {
                    if (@(Get-ChildItem -LiteralPath $createdDirectories[$i] -Force).Count -eq 0) { Remove-Item -LiteralPath $createdDirectories[$i] }
                }
            }
        } catch { throw "Activation failed: $original`nRecovery incomplete: $($_.Exception.Message). Preserve metadata and run -Mode Recover." }
        throw "Activation failed and prior connections restored: $original"
    }
    if ($nextState) {
        $verified
        Write-Host 'Connections and neutral CLI startup verified. Open a new PowerShell terminal for the persistent command path.'
    } else { [pscustomobject]@{ status = 'Disconnected'; preservedPreexistingLinks = @($state.links | Where-Object { -not $_.owned }).Count } }
}

function Invoke-HarnessInstall {
    [CmdletBinding()]
    param([string] $SourceRoot, [string] $CodexHome, [string] $UserHome,
        [string] $CodexCommand, [string] $Mode = 'Install', [string] $PathScope = 'User', [switch] $Preview, [switch] $IncludeCodeTools, [switch] $DeferCommit, [string] $DependencyUserHome)
    if (Get-Variable -Name CodexHarnessInstallActive -Scope Global -ValueOnly -ErrorAction SilentlyContinue) { throw 'Another harness operation is active in this PowerShell process.' }
    $identity = [Text.Encoding]::UTF8.GetBytes((Get-HarnessFullPath $UserHome).ToLowerInvariant())
    $lockName = 'Local\CodexHarness-' + [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($identity))
    $mutex = [Threading.Mutex]::new($false, $lockName)
    $acquired = $false
    try {
        try { $acquired = $mutex.WaitOne(0) } catch [Threading.AbandonedMutexException] { $acquired = $true }
        if (-not $acquired) { throw 'Another harness operation is active for this user. Wait for it to finish.' }
        $global:CodexHarnessInstallActive = $true
        Invoke-HarnessInstallCore @PSBoundParameters
    } finally {
        if ($acquired) { $mutex.ReleaseMutex() }
        Remove-Variable -Name CodexHarnessInstallActive -Scope Global -ErrorAction SilentlyContinue
        $mutex.Dispose()
    }
}

Export-ModuleMember -Function Invoke-HarnessInstall, Get-HarnessInventory, Set-HarnessNativeFeature, Get-HarnessNativeFeatures
