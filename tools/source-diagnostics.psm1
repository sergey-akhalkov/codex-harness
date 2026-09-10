#requires -Version 7.4
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-DiagnosticValue($Config, [string]$Key) {
    $value = $Config
    foreach ($part in $Key.Split('.')) {
        if ($value -isnot [Collections.IDictionary] -or -not $value.Contains($part)) { return $null }
        $value = $value[$part]
    }
    return $value
}

function Get-DiagnosticSource($Source) {
    if (-not $Source) { return $null }
    $result = [ordered]@{ type = $Source.type }
    foreach ($key in @('file','dotCodexFolder','profile')) {
        if ($Source.Contains($key)) { $result[$key] = $Source[$key] }
    }
    return $result
}

function Get-DiagnosticPreference([string]$Key, $Value) {
    if ($null -eq $Value) { return $null }
    if ($Key -eq 'developer_instructions') { return '[present; content withheld]' }
    if ($Value -is [bool]) { return $Value }
    $allowed = switch ($Key) {
        'model' { '^(gpt-6-astra|xai/grok-4\.[0-9]+(?:-[a-z0-9-]+)?)$' }
        'model_reasoning_effort' { '^(none|minimal|low|medium|high|xhigh|max|ultra)$' }
        'approval_policy' { '^(never|untrusted|on-request|on-failure)$' }
        'sandbox_mode' { '^(read-only|workspace-write|danger-full-access)$' }
        default { 'a^' }
    }
    if ($Value -is [string] -and $Value -cmatch $allowed) { return $Value }
    return '[value withheld]'
}

function Invoke-DiagnosticRpc($Server, [string]$Method, $Parameters) {
    $id = $Server.nextId++
    $Server.process.StandardInput.WriteLine((@{id=$id;method=$Method;params=$Parameters} | ConvertTo-Json -Depth 20 -Compress))
    $Server.process.StandardInput.Flush()
    while ($true) {
        $remaining = [int]($Server.deadline - [DateTime]::UtcNow).TotalMilliseconds
        if ($remaining -le 0) { throw [TimeoutException]::new('native-timeout') }
        $line = $Server.process.StandardOutput.ReadLineAsync()
        if (-not $line.Wait($remaining)) { throw [TimeoutException]::new('native-timeout') }
        if ($null -eq $line.Result) { throw 'native-exited' }
        $response = $line.Result | ConvertFrom-Json -AsHashtable
        if ($response.ContainsKey('id') -and $response.id -eq $id) {
            if ($response.ContainsKey('error')) { throw 'native-request-rejected' }
            return $response.result
        }
    }
}

function Start-DiagnosticConsumer([string]$Native, [string]$ConfigHome, [string]$Directory, [DateTime]$Deadline, [string[]]$Overrides = @()) {
    $start=[Diagnostics.ProcessStartInfo]::new($Native)
    foreach ($argument in $Overrides) { $start.ArgumentList.Add($argument) }
    foreach ($argument in @('app-server','--stdio')) { $start.ArgumentList.Add($argument) }
    $start.WorkingDirectory=$Directory; $start.UseShellExecute=$false; $start.CreateNoWindow=$true
    $start.RedirectStandardInput=$true; $start.RedirectStandardOutput=$true; $start.RedirectStandardError=$true
    $start.StandardInputEncoding=[Text.UTF8Encoding]::new($false)
    $start.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
    $start.Environment['CODEX_HOME']=$ConfigHome
    $process=[Diagnostics.Process]::Start($start)
    $drain=$process.StandardError.BaseStream.CopyToAsync([IO.Stream]::Null)
    @{process=$process;nextId=1;deadline=$Deadline;drain=$drain}
}

function Stop-DiagnosticConsumer($Server) {
    if (-not $Server) { return }
    $process=$Server.process
    try {
        try { $process.StandardInput.Close() } catch { }
        if (-not $process.WaitForExit(2000)) { $process.Kill($true); $null=$process.WaitForExit(3000) }
    } finally { $process.Dispose() }
}

function Initialize-DiagnosticConsumer($Server) {
    $result=Invoke-DiagnosticRpc $Server 'initialize' @{clientInfo=@{name='codex-harness-source-check';version='1'};capabilities=@{experimentalApi=$true}}
    $Server.process.StandardInput.WriteLine('{"method":"initialized"}')
    $Server.process.StandardInput.Flush()
    return $result
}

function Read-DiagnosticProfile([string]$Native,[string]$ProfilePath,[DateTime]$Deadline) {
    # Native parsing only. The original file stays linked; no configuration body is copied.
    $temporary=Join-Path ([IO.Path]::GetTempPath()) ('harness-profile-read-'+[guid]::NewGuid().ToString('N'))
    $server=$null
    try {
        $null=New-Item -ItemType Directory -Path $temporary
        $null=New-Item -ItemType SymbolicLink -Path (Join-Path $temporary 'config.toml') -Target $ProfilePath
        $server=Start-DiagnosticConsumer $Native $temporary ([IO.Path]::GetTempPath()) $Deadline
        $null=Initialize-DiagnosticConsumer $server
        $read=Invoke-DiagnosticRpc $server 'config/read' @{includeLayers=$true;cwd=$temporary}
        $userLayers=@($read.layers | Where-Object { $_.name.type -eq 'user' })
        if ($userLayers.Count -ne 1) { throw 'profile-layer-unavailable' }
        return $userLayers[0].config
    } finally {
        Stop-DiagnosticConsumer $server
        # Remove the source link first; the remaining directory is diagnostic-owned runtime state.
        $link=Join-Path $temporary 'config.toml'
        if (Get-Item -LiteralPath $link -Force -ErrorAction SilentlyContinue) { Remove-Item -LiteralPath $link -Force }
        $full=[IO.Path]::GetFullPath($temporary)
        if (-not $full.StartsWith([IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\')+'\harness-profile-read-',[StringComparison]::OrdinalIgnoreCase)) { throw 'cleanup-root-invalid' }
        if (Test-Path -LiteralPath $full) {
            for ($attempt=0; ; $attempt++) {
                try { Remove-Item -LiteralPath $full -Recurse -Force; break }
                catch [IO.IOException] {
                    if ($attempt -ge 9) { throw 'profile-cleanup-failed' }
                    Start-Sleep -Milliseconds 100
                }
            }
        }
    }
}

function Invoke-HarnessSourceDiagnostics {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$SourceRoot,
        [Parameter(Mandatory)][string]$CodexHome,
        [string]$UserHome = [Environment]::GetFolderPath('UserProfile'),
        [string]$ProjectPath = (Get-Location).Path, [string]$CodexCommand,
        [string]$ProfileName = 'harness', [ValidateRange(1,60)][int]$TimeoutSeconds = 30)
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $findings = [Collections.Generic.List[object]]::new()
    $links = [Collections.Generic.List[object]]::new()
    $report = [ordered]@{ schemaVersion=1; status='incomplete'; project=[IO.Path]::GetFullPath($ProjectPath);
        codexHome=[IO.Path]::GetFullPath($CodexHome); sourceRoot=[IO.Path]::GetFullPath($SourceRoot); profile=$ProfileName;
        observation='native-base-and-profile-layers'; native=@{status='unavailable'}; settings=@(); layers=@(); skills=@();
        links=@(); findings=@(); freshness=@{existingSessions='unknown'; existingMcpLspServers='unknown';
            action='Restart the affected consumer to load source changes; use ordinary Check for protocol health.'} }
    $state = $null
    try {
        $statePath = Join-Path $CodexHome 'harness/installation.json'
        $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json -AsHashtable
        if ($state.schemaVersion -ne 1 -or -not $state.ContainsKey('links') -or -not $state.ContainsKey('codexCommand')) { throw 'state-schema' }
    } catch {
        $state=$null
        $findings.Add(@{code='installation-unavailable';source=(Join-Path $CodexHome 'harness/installation.json');action='Install or recover the kit for this Codex home.'})
    }
    foreach ($pending in @('pending.json','activation-pending.json')) {
        $path = Join-Path $CodexHome "harness/$pending"
        if (Test-Path -LiteralPath $path) { $findings.Add(@{code='pending-transaction';source=$path;action='Finish or recover the recorded installation before changing links.'}) }
    }
    $expected = @{}
    if ($state) {
        foreach ($link in $state.links) { $expected[$link.destination] = $link }
        if (-not $CodexCommand) { $CodexCommand = $state.codexCommand }
    }
    try {
        Import-Module (Join-Path $PSScriptRoot 'kit.psm1') -Force
        $inventory = Get-HarnessInventory $SourceRoot $CodexHome $UserHome
        foreach ($link in $inventory.links) { $expected[$link.destination] = $link }
    } catch {
        $findings.Add(@{code='inventory-unavailable';source=$SourceRoot;action='Restore the complete kit checkout and rerun Check.'})
    }
    foreach ($link in @($expected.Values | Sort-Object destination)) {
        $entry = [ordered]@{kind=$link.kind;destination=$link.destination;expected=$link.source;actual=$null;status='missing'}
        try {
            $item = Get-Item -LiteralPath $link.destination -Force -ErrorAction Stop
            if ($item.LinkType) { $entry.actual = $item.ResolveLinkTarget($true).FullName }
            $entry.status = if (-not $entry.actual) { 'not-a-link' }
                elseif (-not [string]::Equals([IO.Path]::GetFullPath($entry.actual),[IO.Path]::GetFullPath($link.source),[StringComparison]::OrdinalIgnoreCase)) { 'retargeted' }
                elseif (-not (Test-Path -LiteralPath $entry.actual)) { 'missing-source' } else { 'connected' }
        } catch { $entry.status = 'missing-or-unreadable' }
        $links.Add($entry)
        if ($entry.status -ne 'connected') { $findings.Add(@{code='link-'+$entry.status;source=$entry.destination;action='Resolve ownership of this destination, then run install.ps1 -Mode Update from the intended checkout.'}) }
    }
    $overridePath = Join-Path $CodexHome 'AGENTS.override.md'
    if (Test-Path -LiteralPath $overridePath) { $findings.Add(@{code='instructions-hidden';source=$overridePath;action='Review the global override that hides AGENTS.md; compose or remove it explicitly.'}) }
    $server = $null
    try {
        if (-not (Test-Path -LiteralPath $report.project -PathType Container)) { throw 'project-unavailable' }
        if (-not $CodexCommand) { throw 'native-unavailable' }
        # Resolve only the known npm package layout; never search a project or execute its commands.
        $native = $CodexCommand
        if ([IO.Path]::GetExtension($native) -ne '.exe') {
            $vendor = Join-Path (Split-Path $native) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
            $candidates = @(Get-ChildItem -LiteralPath $vendor -Recurse -File -Filter codex.exe)
            if ($candidates.Count -ne 1) { throw 'native-unavailable' }
            $native = $candidates[0].FullName
        }
        if ($ProfileName -cnotmatch '^[a-zA-Z0-9_-]+$') { throw 'invalid-profile-name' }
        $deadline=[DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
        $sharedConsumer = $ProfileName -ceq 'harness'
        [string[]]$overrides = @()
        if ($sharedConsumer) {
            $registration = Get-Content -LiteralPath (Join-Path $CodexHome 'harness/installation.json') -Raw | ConvertFrom-Json
            $encoded = & $registration.configBridge config-overrides --source $SourceRoot --codex-home $CodexHome
            if ($LASTEXITCODE -ne 0) { throw 'shared-config-unavailable' }
            $overrides = @($encoded | ConvertFrom-Json)
        }
        $server=Start-DiagnosticConsumer $native $CodexHome ([IO.Path]::GetTempPath()) $deadline $overrides
        $init=Initialize-DiagnosticConsumer $server
        $nativeVersion=[regex]::Match($init.userAgent,'/([0-9]+\.[0-9]+\.[0-9]+)').Groups[1].Value
        if ($nativeVersion -notin @('0.153.4','0.154.0')) { throw 'unverified-profile-precedence-contract' }
        $read = Invoke-DiagnosticRpc $server 'config/read' @{includeLayers=$true;cwd=$report.project}
        if (-not $read.ContainsKey('origins') -or -not $read.ContainsKey('layers')) { throw 'unsupported-contract' }
        $requirements=Invoke-DiagnosticRpc $server 'configRequirements/read' @{}
        $profilePath=if ($sharedConsumer) { Join-Path $SourceRoot 'global/harness.config.toml' } else { Join-Path $CodexHome ($ProfileName+'.config.toml') }
        $profileConfig=if ($sharedConsumer) { @{} } else { Read-DiagnosticProfile $native $profilePath $deadline }
        # These inputs can change project discovery/trust or skill selection before merging.
        $contextChanged=$false
        foreach ($key in @('project_root_markers','credential_broker','skills')) {
            if ($profileConfig.ContainsKey($key)) { $contextChanged=$true }
        }
        if ($profileConfig.ContainsKey('projects')) {
            foreach ($path in $profileConfig.projects.Keys) {
                $trustPath=[IO.Path]::GetFullPath($path).TrimEnd('\')
                if ($report.project -eq $trustPath -or $report.project.StartsWith($trustPath+'\',[StringComparison]::OrdinalIgnoreCase)) { $contextChanged=$true }
            }
        }
        if ($contextChanged) { $findings.Add(@{code='profile-context-unresolved';source=$profilePath;action='The profile can alter discovery, trust or skill loading. Use the selected-profile CLI to inspect this project; no effective settings are asserted.'}) }
        if ($null -ne $requirements.requirements) { $findings.Add(@{code='runtime-requirements-present';source='native requirements';action='Merged declarations may be constrained by managed requirements; check the selected-profile runtime before relying on these values.'}) }
        $layers=[Collections.Generic.List[object]]::new()
        $inserted=$false
        # config/read returns highest precedence first; merge declarations lowest first.
        $baseLayers=@($read.layers)
        [array]::Reverse($baseLayers)
        foreach ($layer in $baseLayers) {
            $layers.Add($layer)
            if ($layer.name.type -eq 'user') {
                $layers.Add(@{name=@{type='user';file=$profilePath;profile=$ProfileName};config=$profileConfig})
                $inserted=$true
            }
        }
        if (-not $inserted) { throw 'missing-native-user-layer' }
        $settings = [Collections.Generic.List[object]]::new()
        $safeKeys=@('model','model_reasoning_effort','approval_policy','sandbox_mode','developer_instructions','features.hooks','features.multi_agent','features.memories')
        foreach ($key in $safeKeys) {
            $declarations = @($layers | Where-Object { $null -ne (Get-DiagnosticValue $_.config $key) } | ForEach-Object {
                @{source=(Get-DiagnosticSource $_.name);active=(-not $_['disabledReason']);value=(Get-DiagnosticPreference $key (Get-DiagnosticValue $_.config $key))}
            })
            $winner=@($declarations | Where-Object active | Select-Object -Last 1)
            $origin=if ($winner.Count -and -not $contextChanged -and $null -eq $requirements.requirements) {$winner[0].source} else {$null}
            $profileLayer = @($declarations | Where-Object { $_.active -and $_.source.Contains('profile') -and $_.source.profile -eq $ProfileName })
            $overridden = $profileLayer.Count -gt 0 -and $null -ne $origin -and (-not $origin.Contains('profile') -or $origin.profile -ne $ProfileName)
            $settings.Add(@{key=$key;value=$(if ($origin) {$winner[0].value} else {$null});origin=$origin;declarations=$declarations;overridden=$overridden;evidence='inferred-from-native-layers';runtimeDefault='not-inferred'})
            if ($overridden) { $findings.Add(@{code='setting-overridden';key=$key;source=$origin;action='Review the winning source; retain an intentional override or edit it explicitly to use the profile.'}) }
        }
        $report.settings=$settings.ToArray()
        $report.layers=@($layers | ForEach-Object { @{source=(Get-DiagnosticSource $_.name);status=$(if ($_['disabledReason']) {'disabled'} else {'active'})} })
        $read=$null
        $listed = Invoke-DiagnosticRpc $server 'skills/list' @{cwds=@($report.project);forceReload=$true}
        $skillRows = [Collections.Generic.List[object]]::new()
        foreach ($entry in $listed.data) {
            foreach ($skill in $entry.skills) { $skillRows.Add(@{name=$skill.name;path=$skill.path;enabled=$skill.enabled;scope=$skill.scope}) }
            if (@($entry.errors).Count) { $findings.Add(@{code='skill-load-error';source=$entry.cwd;action='Inspect skill frontmatter locally; native error text is withheld.'}) }
        }
        $report.skills = @($skillRows.ToArray() | Sort-Object name,path -Unique)
        foreach ($group in @($report.skills | Where-Object enabled | Group-Object name | Where-Object Count -gt 1)) {
            $findings.Add(@{code='skill-name-collision';name=$group.Name;sources=@($group.Group.path);action='Rename or explicitly disable the unintended skill source; same-name skills are ambiguous.'})
        }
        $report.native=@{status='observed';executable=$native;protocol='config/read + configRequirements/read + skills/list';version=$nativeVersion;profileSelection=$(if ($sharedConsumer) {'live shared CLI overrides; native base persistence'} else {'reconstructed; app-server does not accept file profiles'});skillsScope='native base consumer'}
    } catch {
        $category = if ($_.Exception -is [TimeoutException]) {'native-timeout'} else {'native-unavailable-or-incompatible'}
        $findings.Add(@{code=$category;source=$CodexCommand;action='Check the installed CLI, selected profile, project path and TOML locally; native error text is withheld.'})
    } finally {
        Stop-DiagnosticConsumer $server
    }
    $report.links=$links.ToArray(); $report.findings=$findings.ToArray()
    $report.status = if ($report.native.status -ne 'observed' -or @($findings | Where-Object code -in @('installation-unavailable','inventory-unavailable','skill-load-error','profile-context-unresolved','runtime-requirements-present')).Count) {'incomplete'} elseif ($findings.Count) {'attention'} else {'healthy'}
    $report.elapsedMilliseconds=$watch.ElapsedMilliseconds
    [pscustomobject]$report
}

Export-ModuleMember -Function Invoke-HarnessSourceDiagnostics
