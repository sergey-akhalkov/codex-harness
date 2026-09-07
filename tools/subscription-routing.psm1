#requires -Version 7.4
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'code-tools.psm1')

function Get-SubscriptionPaths([string]$SourceRoot, [string]$UserHome, [string]$CodexHome) {
    $SourceRoot = [IO.Path]::GetFullPath($SourceRoot); $UserHome = [IO.Path]::GetFullPath($UserHome); $CodexHome = [IO.Path]::GetFullPath($CodexHome)
    $identity = (Get-CodeToolsHash ([Text.Encoding]::UTF8.GetBytes($CodexHome.ToLowerInvariant()))).Substring(0,16)
    @{ source = $SourceRoot; user = $UserHome; codex = $CodexHome; opencodex = Join-Path $UserHome '.opencodex'
        state = Join-Path $CodexHome 'harness/subscription-routing.json'; pending = Join-Path $CodexHome 'harness/subscription-routing-pending.json'
        service = Join-Path $CodexHome 'harness/subscriptions/service.json'; runtime = Join-Path $CodexHome 'harness/subscriptions/runs'
        config = Join-Path $CodexHome 'config.toml'; configLink = Join-Path $UserHome '.opencodex/config.json'
        configSource = Join-Path $SourceRoot 'global/opencodex/config.json'; roleLink = Join-Path $CodexHome 'agents/codex-harness-subscriptions'
        roleSource = Join-Path $SourceRoot 'global/opencodex/agents'; task = 'codex-harness-subscriptions-' + $identity }
}
function Get-SubscriptionLink([string]$Path) {
    Assert-CodeToolsPlain (Split-Path $Path)
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    if (-not $item) { return $null }
    if ($item.LinkType -ne 'SymbolicLink' -or -not $item.LinkTarget) { throw "Foreign connection preserved: $Path" }
    [IO.Path]::GetFullPath($item.LinkTarget, (Split-Path $Path))
}
function Set-SubscriptionLink([string]$Path, [AllowNull()][string]$Target, [AllowNull()][string]$Expected) {
    $actual = [string](Get-SubscriptionLink $Path)
    if ($actual -ne $Expected) { throw "Subscription link changed; preserving: $Path" }
    if ($actual -eq $Target) { return }
    if ($actual) { Remove-Item -LiteralPath $Path -Force }
    if ($Target) {
        [void][IO.Directory]::CreateDirectory((Split-Path $Path))
        New-Item -ItemType SymbolicLink -Path $Path -Target $Target -ErrorAction Stop | Out-Null
    }
}
function Get-SubscriptionSnapshot([string]$Path) {
    Assert-CodeToolsPlain $Path
    if (Test-Path -LiteralPath $Path) { return [Convert]::ToBase64String((Get-CodeToolsBytes $Path)) }
    $null
}
function Restore-SubscriptionSnapshot([string]$Path, $Before, $After, $Native = $Before) {
    $current = Get-SubscriptionSnapshot $Path
    if ($current -cne $Before -and $current -cne $After -and $current -cne $Native) { throw "Subscription file changed; preserving: $Path" }
    if ($null -eq $Before) { Remove-CodeToolsFile $Path } else { Write-CodeToolsBytes $Path ([Convert]::FromBase64String($Before)) }
}
function Get-SubscriptionTask([string]$Name) {
    $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect()
    try { $task = $scheduler.GetFolder('\').GetTask($Name) }
    catch {
        $failure = $_.Exception
        while ($failure) { if ($failure.HResult -eq -2147024894) { return $null }; $failure = $failure.InnerException }
        throw
    }
    @{ xml = $task.Xml; running = ($task.State -eq 4); name = $Name; task_state = [int]$task.State
        last_result = [long]$task.LastTaskResult; last_run = ([datetime]$task.LastRunTime).ToUniversalTime().ToString('o') }
}
function Get-SubscriptionOwnedProcess($Paths) {
    $started = Get-SubscriptionStarted $Paths
    if (-not $started) { return $null }
    $process = $null
    try {
        $process = [Diagnostics.Process]::GetProcessById([int]$started.processId)
        # Open the process handle before Stop, so PID reuse cannot redirect the wait.
        [void]$process.Handle
        return $process
    } catch [ArgumentException] { if ($process) { $process.Dispose() }; return $null }
    catch { if ($process) { $process.Dispose() }; throw }
}
function Stop-SubscriptionTaskRuntime($Task, $Paths) {
    $process = Get-SubscriptionOwnedProcess $Paths
    try {
        $deadline = [DateTime]::UtcNow.AddSeconds(10)
        $Task.Stop(0)
        while ($Task.State -eq 4 -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
        if ($Task.State -eq 4) { throw 'Owned task did not stop; native routing is preserved for recovery.' }
        # Scheduler state can change before job-close finishes terminating Bun.
        # Wait only here, after stopping our task; marker cleanup still rejects live PIDs.
        $remaining = [int][Math]::Max(0, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        if ($process -and -not $process.WaitForExit($remaining)) {
            throw 'Owned subscription runtime did not stop within 10 seconds; task and native routing are preserved for recovery.'
        }
    } finally { if ($process) { $process.Dispose() } }
}
function Set-SubscriptionTask([string]$Name, [AllowNull()][string]$Xml, [AllowNull()][string]$ExpectedXml, $Paths) {
    $current = Get-SubscriptionTask $Name
    if (($current -and $current.xml -cne $ExpectedXml) -or (-not $current -and $ExpectedXml)) { throw 'Scheduled task changed; preserving foreign definition.' }
    $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect(); $folder = $scheduler.GetFolder('\')
    if ($current) {
        $task = $folder.GetTask($Name)
        Stop-SubscriptionTaskRuntime $task $Paths
        $folder.DeleteTask($Name,0)
    }
    if ($Xml) { $null = $folder.RegisterTask($Name,$Xml,2,$null,$null,3,$null) }
}
function Start-SubscriptionTask([string]$Name) {
    $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect()
    $null = $scheduler.GetFolder('\').GetTask($Name).Run($null)
}
function New-SubscriptionTaskXml($Paths, [string]$PowerShell) {
    $scheduler = New-Object -ComObject 'Schedule.Service'; $scheduler.Connect(); $definition = $scheduler.NewTask(0)
    $definition.RegistrationInfo.Description = 'codex-harness subscription routing: ' + $Paths.codex
    $definition.RegistrationInfo.URI = '\' + $Paths.task
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $definition.Principal.UserId = $identity.User.Value
        # Match the installer's existing token. An elevated install may need
        # this same authority to recreate role symlinks after a task restart.
        $definition.Principal.RunLevel = if ([Security.Principal.WindowsPrincipal]::new($identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { 1 } else { 0 }
    } finally { $identity.Dispose() }
    $definition.Principal.LogonType = 3
    $definition.Settings.Enabled = $true; $definition.Settings.Hidden = $true
    $definition.Settings.ExecutionTimeLimit = 'PT0S'; $definition.Settings.RestartCount = 0
    $definition.Settings.MultipleInstances = 2; $definition.Settings.StartWhenAvailable = $true
    $definition.Settings.DisallowStartIfOnBatteries = $false; $definition.Settings.StopIfGoingOnBatteries = $false
    $trigger = $definition.Triggers.Create(9); $trigger.UserId = $definition.Principal.UserId
    $action = $definition.Actions.Create(0); $action.Path = $PowerShell
    $action.Arguments = '-NoLogo -NoProfile -WindowStyle Hidden -File "' + (Join-Path $Paths.source 'tools/opencodex-service.ps1') + '" -StatePath "' + $Paths.service + '"'
    $action.WorkingDirectory = $Paths.source
    $definition.XmlText
}
function Resolve-SubscriptionPowerShell {
    # Store/MSIX pwsh can fail before script entry under Task Scheduler. Reuse
    # an installed native host without changing interactive command selection.
    $candidates = @((Join-Path $env:ProgramFiles 'PowerShell/7/pwsh.exe'))
    $candidates += @(Get-Command pwsh -CommandType Application -All -ErrorAction SilentlyContinue | ForEach-Object Source)
    foreach ($candidate in $candidates | Select-Object -Unique) {
        if ($candidate -match '(?i)\\WindowsApps\\' -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
        Assert-CodeToolsPlain $candidate
        $version = [Diagnostics.FileVersionInfo]::GetVersionInfo($candidate)
        if ($version.FileMajorPart -gt 7 -or ($version.FileMajorPart -eq 7 -and $version.FileMinorPart -ge 4)) { return $candidate }
    }
    throw 'Subscription background startup requires native PowerShell 7.4+ (MSI/portable); Microsoft Store PowerShell is not a supported scheduled host.'
}
function Get-SubscriptionDependency($Paths, [string]$CodexCommand) {
    $declaration = Read-CodeToolsJson (Join-Path $Paths.source 'global/opencodex/dependency.json')
    if (-not $declaration -or $declaration.package -ne '@bitkyc08/opencodex' -or $declaration.version -ne '2.44.0') { throw 'Subscription dependency declaration is missing or unaudited.' }
    $npm = Get-Command npm -ErrorAction SilentlyContinue
    $roots = @((Join-Path $Paths.codex 'harness/dependencies/opencodex/node_modules/@bitkyc08/opencodex'))
    if ($npm) { $roots += Join-Path (Split-Path $npm.Source) 'node_modules/@bitkyc08/opencodex' }
    if ($CodexCommand) { $roots += Join-Path (Split-Path $CodexCommand) 'node_modules/@bitkyc08/opencodex' }
    foreach ($root in $roots | Select-Object -Unique) {
        $metadata = Read-CodeToolsJson (Join-Path $root 'package.json')
        if ($metadata -and $metadata.name -eq $declaration.package -and $metadata.version -eq $declaration.version) {
            $bun = Join-Path $root 'node_modules/bun/bin/bun.exe'; $cli = Join-Path $root 'src/cli/index.ts'
            if ((Test-Path -LiteralPath $bun) -and (Test-Path -LiteralPath $cli)) { return @{ root = $root; bun = $bun; cli = $cli; version = $metadata.version; powershell = Resolve-SubscriptionPowerShell } }
        }
    }
    $null
}
function Invoke-SubscriptionBounded($Paths, [string]$Executable, [string[]]$Arguments, [hashtable]$Environment = @{}, [int]$Timeout = 30, [switch]$ServiceRun, [scriptblock]$OnRunning) {
    Assert-CodeToolsPlain $Paths.runtime; [void][IO.Directory]::CreateDirectory($Paths.runtime)
    $prefix = Join-Path $Paths.runtime ([DateTime]::UtcNow.ToString('yyyyMMddTHHmmss') + '-' + [guid]::NewGuid().ToString('N'))
    $request = @{ executable = $Executable; arguments = $Arguments; workingDirectory = $Paths.source
        stdoutPath = $prefix + '.stdout'; stderrPath = $prefix + '.stderr'; environment = $Environment; memoryLimitMiB = 768; timeoutSeconds = $Timeout }
    if ($ServiceRun) {
        # Real long-context traffic exhausted 768 MiB twice. Keep containment,
        # with headroom for concurrent requests and response-state serialization.
        # Short administrative commands and OAuth retain their smaller allowance.
        $request.memoryLimitMiB = 2048
        # Retire a stopped run while active-run still points at its receipt.
        # Overwriting that proof first can strand stale upstream PID markers.
        Remove-SubscriptionStoppedMarkers $Paths
        $request.startedPath = $prefix + '.started.json'
        Write-CodeToolsJson (Join-Path $Paths.runtime 'active-run.json') @{ started = $request.startedPath }
    }
    Write-CodeToolsJson ($prefix + '.request.json') $request
    $result = & (Join-Path $Paths.source 'tools/opencodex-process.ps1') -RequestPath ($prefix + '.request.json') -ResultPath ($prefix + '.result.json') -PassThru -OnRunning $OnRunning
    if ($result.ExitCode -ne 0) { throw "Bounded subscription command failed ($($result.Status), exit $($result.ExitCode)); private logs: $prefix" }
    @{ result = $result; stdout = $request.stdoutPath }
}
function Initialize-SubscriptionDependency($Paths, [string]$CodexCommand) {
    $dependency = Get-SubscriptionDependency $Paths $CodexCommand
    if ($dependency) { return $dependency }
    # Install into a dedicated host prefix. Existing global packages are never replaced.
    $prefix = Join-Path $Paths.codex 'harness/dependencies/opencodex'
    if (Test-Path -LiteralPath $prefix) { throw 'Incomplete private OpenCodex dependency preserved; inspect it before retrying installation.' }
    $node = (Get-Command node -CommandType Application -ErrorAction Stop).Source
    $npmCli = Join-Path (Split-Path $node) 'node_modules/npm/bin/npm-cli.js'
    Assert-CodeToolsPlain $prefix
    Invoke-SubscriptionBounded $Paths $node @($npmCli,'install','--prefix',$prefix,'--no-audit','--no-fund','@bitkyc08/opencodex@2.44.0') -Timeout 180 | Out-Null
    $dependency = Get-SubscriptionDependency $Paths $CodexCommand
    if (-not $dependency) { throw 'Private package installation did not produce the audited runtime; preserve the dependency prefix for inspection.' }
    $dependency
}
function Restore-SubscriptionNative($Paths, $Dependency) {
    Assert-SubscriptionRuntimeOwned $Paths
    Remove-SubscriptionStoppedMarkers $Paths
    $environment = @{ CODEX_HOME = $Paths.codex; OPENCODEX_HOME = $Paths.opencodex }
    Invoke-SubscriptionBounded $Paths $Dependency.bun @('--no-env-file',(Join-Path $Paths.source 'tools/opencodex-native-restore.mjs'), $Dependency.root) $environment | Out-Null
}
function Assert-SubscriptionConfiguration($Paths, $Dependency) {
    $environment = @{ CODEX_HOME = $Paths.codex; OPENCODEX_HOME = $Paths.opencodex }
    Invoke-SubscriptionBounded $Paths $Dependency.bun @('--no-env-file',(Join-Path $Paths.source 'tools/opencodex-config-check.mjs'), $Dependency.root, $Paths.source) $environment | Out-Null
}
function Assert-SubscriptionRuntimeOwned($Paths) {
    $runtime = Read-CodeToolsJson (Join-Path $Paths.opencodex 'runtime-port.json')
    if ($runtime) {
        $started = Get-SubscriptionStarted $Paths
        if (-not $started -or $runtime.pid -ne $started.processId) { throw 'Another OpenCodex runtime owns this home; native routing is preserved.' }
    }
}
function Get-SubscriptionStarted($Paths) {
    $active = Read-CodeToolsJson (Join-Path $Paths.runtime 'active-run.json')
    if (-not $active) { return $null }
    if ((Split-Path ([IO.Path]::GetFullPath($active.started))) -ne [IO.Path]::GetFullPath($Paths.runtime)) { throw 'Foreign subscription process receipt path preserved.' }
    $started = Read-CodeToolsJson $active.started
    if ($started -and $started.assignedBeforeResume -eq $true) { return $started }
    $null
}
function Test-SubscriptionProcessAlive([int]$ProcessId) {
    $process = $null
    try {
        $process = [Diagnostics.Process]::GetProcessById($ProcessId)
        return -not $process.HasExited
    } catch [ArgumentException] { return $false }
    finally { if ($process) { $process.Dispose() } }
}
function Remove-SubscriptionStoppedMarkers($Paths) {
    $markers = @()
    foreach ($name in @('runtime-port.json','ocx.pid')) {
        $path = Join-Path $Paths.opencodex $name
        $snapshot = Get-SubscriptionSnapshot $path
        if ($null -eq $snapshot) { continue }
        $content = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($snapshot))
        $value = if ($name -eq 'runtime-port.json') { ($content | ConvertFrom-Json -AsHashtable).pid } else { $content.Trim() }
        $markerPid = 0
        if (-not [int]::TryParse([string]$value, [ref]$markerPid) -or $markerPid -le 0) { throw 'Invalid OpenCodex PID marker preserved.' }
        $markers += @{ path=$path; snapshot=$snapshot; processId=$markerPid }
    }
    if (-not $markers.Count) { return }
    $started = Get-SubscriptionStarted $Paths
    foreach ($marker in $markers) {
        if (-not $started -or $started.assignedBeforeResume -ne $true -or $marker.processId -ne $started.processId) {
            throw 'Another OpenCodex runtime marker owns this home; preserving markers and current receipt.'
        }
        if (Test-SubscriptionProcessAlive $marker.processId) { throw 'Subscription process is still running; preserving its runtime markers.' }
        if ((Get-SubscriptionSnapshot $marker.path) -cne $marker.snapshot) { throw 'OpenCodex runtime marker changed; preserving markers.' }
    }
    foreach ($marker in $markers) {
        if ((Get-SubscriptionSnapshot $marker.path) -cne $marker.snapshot -or (Test-SubscriptionProcessAlive $marker.processId)) {
            throw 'OpenCodex runtime ownership changed; preserving remaining markers.'
        }
        Remove-CodeToolsFile $marker.path
    }
}
function Assert-SubscriptionPortFree([int]$Port) {
    $listener = [Net.Sockets.TcpListener]::new([Net.IPAddress]::Loopback,$Port)
    $listener.Server.ExclusiveAddressUse = $true
    try { $listener.Start() } catch { throw "Port $Port is already in use; existing listener preserved." } finally { $listener.Stop() }
}
function Test-SubscriptionReady($Paths, [int]$Port) {
    try {
        $started = Get-SubscriptionStarted $Paths
        $runtime = Read-CodeToolsJson (Join-Path $Paths.opencodex 'runtime-port.json')
        if (-not $started -or -not $runtime -or $runtime.pid -ne $started.processId -or $runtime.port -ne $Port) { return $false }
        # This owned loopback probe needs neither a system proxy nor a reused
        # connection. The default HTTP client produced false timeouts while
        # direct fresh requests and curl still attested the same live service.
        $response = Invoke-RestMethod ("http://127.0.0.1:$Port/readyz") -NoProxy -DisableKeepAlive -TimeoutSec 2
        return ($response.status -eq 'ready' -and $response.service -eq 'opencodex' -and $response.pid -eq $started.processId -and $response.port -eq $Port)
    } catch { return $false }
}
function Wait-SubscriptionReady($Paths, [int]$Port) {
    $requestedAfter = [DateTime]::UtcNow.AddSeconds(-2)
    $deadline = [DateTime]::UtcNow.AddSeconds(90)
    $taskDetail = 'task absent'
    do {
        $task = Get-SubscriptionTask $Paths.task
        if ($task -and $task.running -and (Test-SubscriptionReady $Paths $Port)) {
            # An independent restart also restores its role after /readyz.
            # During a transaction the installer owns that later link write.
            if ((Read-CodeToolsJson $Paths.pending) -or (Get-SubscriptionLink $Paths.roleLink) -eq $Paths.roleSource) { return }
        }
        if ($task -and $task.ContainsKey('task_state')) {
            $taskDetail = 'state={0}, lastResult=0x{1:X8}, lastRun={2}' -f $task.task_state, ($task.last_result -band 0xffffffffL), $task.last_run
            # A newly requested task may still be queued or have an old result.
            # Only a completed invocation with a fresh run time is terminal.
            if ($task.task_state -eq 3 -and [datetime]$task.last_run -ge $requestedAfter -and $task.last_result -notin @(0x41301,0x41303,0x41325)) {
                throw "Subscription scheduled host exited before readiness ($taskDetail). Private entry/run logs: $($Paths.runtime)"
            }
        }
        Start-Sleep -Milliseconds 250
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Subscription background startup was not ready within 90 seconds ($taskDetail). Private entry/run logs: $($Paths.runtime)"
}
function Assert-SubscriptionState($State, $Paths) {
    if ($State.schema_version -ne 1 -or $State.owner -ne 'codex-harness-subscriptions' -or $State.codex -ne $Paths.codex -or $State.user -ne $Paths.user -or $State.task -ne $Paths.task) { throw 'Subscription ownership record mismatch; preserving state.' }
}
function Update-SubscriptionServiceReadiness($Paths, [int]$Port, $Monitor) {
    if ($Monitor.ready) { return }
    if (Test-SubscriptionReady $Paths $Port) {
        # Install/Recover own their journaled link writes. A later independent
        # logon/task restart reconnects a missing role only after attested ready.
        if (-not (Read-CodeToolsJson $Paths.pending)) {
            $state = Read-CodeToolsJson $Paths.state
            if (-not $state) { throw 'Subscription service has no committed ownership record.' }
            Assert-SubscriptionState $state $Paths
            if ($state.links.roleLink -ne $Paths.roleSource) { throw 'Subscription service role source changed; run Install.' }
            $role = Get-SubscriptionLink $Paths.roleLink
            if ($role -and $role -ne $state.links.roleLink) { throw 'Foreign subscription role preserved during service restart.' }
            Set-SubscriptionLink $Paths.roleLink $Paths.roleSource $role
        }
        $Monitor.ready = $true
    } elseif ([DateTime]::UtcNow -ge $Monitor.deadline) { throw 'Subscription service startup did not become ready within 90 seconds.' }
}
function Invoke-SubscriptionServiceHost {
    param([Parameter(Mandatory)][string]$StatePath)
    $descriptor = Read-CodeToolsJson $StatePath
    $paths = Get-SubscriptionPaths $descriptor.source $descriptor.user $descriptor.codex
    if ([IO.Path]::GetFullPath($StatePath) -ne $paths.service) { throw 'Subscription service descriptor path mismatch.' }
    Assert-SubscriptionState $descriptor $paths
    # Independent task/logon starts bypass the installer checks. The runtime
    # must read the source we validate; reject a replaced link before cleanup
    # can restore native routing or remove a role from an unowned connection.
    if ((Get-SubscriptionLink $paths.configLink) -ne $paths.configSource) {
        throw 'Subscription service configuration link is missing or changed; preserving host state.'
    }
    $dependency = $descriptor.dependency
    Assert-SubscriptionRuntimeOwned $paths
    try {
        # A stopped owned run can leave routing behind. A busy port must still
        # restore that native configuration without touching the other listener.
        Assert-SubscriptionPortFree $descriptor.port
        Assert-SubscriptionConfiguration $paths $dependency
        $environment = @{ CODEX_HOME = $paths.codex; OPENCODEX_HOME = $paths.opencodex; OCX_SERVICE = '1' }
        $monitor = @{ ready = $false; deadline = [DateTime]::UtcNow.AddSeconds(90) }
        $observer = { Update-SubscriptionServiceReadiness $paths $descriptor.port $monitor }
        Invoke-SubscriptionBounded $paths $dependency.bun @('--no-env-file',$dependency.cli,'start','--port',[string]$descriptor.port) $environment -Timeout 0 -ServiceRun -OnRunning $observer | Out-Null
    } finally {
        # Task Scheduler Stop can forcibly terminate this PowerShell host. Its
        # caller performs this same restoration after the job has closed.
        try {
            Restore-SubscriptionNative $paths $dependency
            $role = Get-SubscriptionLink $paths.roleLink
            if ($role -eq $paths.roleSource) { Set-SubscriptionLink $paths.roleLink $null $role }
        } catch { Write-CodeToolsJson (Join-Path $paths.runtime 'recovery-required.json') @{ message = $_.Exception.Message; action = 'Run install.ps1 -Mode Recover, then Install.' }; throw }
    }
}
function Complete-HarnessSubscriptionRouting([string]$CodexHome) {
    Remove-CodeToolsFile (Join-Path $CodexHome 'harness/subscription-routing-pending.json')
}
function Restore-HarnessSubscriptionRouting {
    param([string]$SourceRoot,[string]$UserHome,[string]$CodexHome,[string]$CodexCommand,[string]$DependencyUserHome,[switch]$Preview,[switch]$DeferRestart)
    $paths = Get-SubscriptionPaths $SourceRoot $UserHome $CodexHome
    $pending = Read-CodeToolsJson $paths.pending
    if (-not $pending) {
        $state = Read-CodeToolsJson $paths.state
        if ($state) {
            Assert-SubscriptionState $state $paths
            $task = Get-SubscriptionTask $paths.task
            if ($task -and $task.xml -cne $state.task_xml) { throw 'Changed subscription task preserved during recovery.' }
            if (-not $task -or -not $task.running) {
                if (-not $Preview) {
                    Restore-SubscriptionNative $paths $state.dependency
                    $role = Get-SubscriptionLink $paths.roleLink
                    if ($role -and $role -ne $state.links.roleLink) { throw 'Foreign role link preserved during recovery.' }
                    if ($role) { Set-SubscriptionLink $paths.roleLink $null $role }
                }
                return @{ status = 'native-recovered-subscriptions-stopped'; action = 'Install to reconnect subscriptions.' }
            }
        }
        return @{ status = 'no-pending-subscriptions' }
    }
    Assert-SubscriptionState $pending $paths
    if ($pending.ContainsKey('phase') -and $pending.phase -eq 'recovered') {
        if (-not $Preview -and -not $DeferRestart) { Resume-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome }
        return @{ status = 'subscriptions-recovered' }
    }
    $task = Get-SubscriptionTask $paths.task
    $taskXml = if ($task) { $task.xml } else { $null }
    if ($taskXml -and $taskXml -cne $pending.task_before -and $taskXml -cne $pending.task_after) { throw 'Subscription task changed after interruption; preserving.' }
    foreach ($name in @('configLink','roleLink')) {
        $actual = Get-SubscriptionLink $paths[$name]
        if ($actual -ne $pending.links_before[$name] -and $actual -ne $pending.links_after[$name]) { throw 'Subscription connection changed after interruption; preserving.' }
    }
    if ($Preview) { return @{ status = 'preview-subscription-recovery' } }
    if ($task) { Set-SubscriptionTask $paths.task $null $taskXml $paths }
    if ($pending.runtime_started -and (Get-SubscriptionStarted $paths)) { Restore-SubscriptionNative $paths $pending.dependency }
    Restore-SubscriptionSnapshot $paths.config $pending.config_before $pending.config_after $pending.config_native
    foreach ($name in @('roleLink','configLink')) { Set-SubscriptionLink $paths[$name] $pending.links_before[$name] (Get-SubscriptionLink $paths[$name]) }
    Restore-SubscriptionSnapshot $paths.service $pending.service_before $pending.service_after
    Restore-SubscriptionSnapshot $paths.state $pending.state_before $pending.state_after
    if ($pending.task_before) {
        Set-SubscriptionTask $paths.task $pending.task_before $null $paths
    }
    if ($pending.package_attempted) { throw 'Private package acquisition was interrupted before verification; native connections recovered. Preserve the dependency prefix and pending journal for inspection.' }
    $pending.phase = 'recovered'; Write-CodeToolsJson $paths.pending $pending
    if (-not $DeferRestart) { Resume-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome }
    @{ status = 'subscriptions-recovered' }
}
function Resume-HarnessSubscriptionRouting {
    param([string]$SourceRoot,[string]$UserHome,[string]$CodexHome)
    $paths = Get-SubscriptionPaths $SourceRoot $UserHome $CodexHome
    $pending = Read-CodeToolsJson $paths.pending
    if (-not $pending) { return }
    Assert-SubscriptionState $pending $paths
    if (-not $pending.ContainsKey('phase') -or $pending.phase -ne 'recovered') { throw 'Subscription recovery has not completed its file phase.' }
    if ($pending.task_before -and $pending.running_before) {
        $task = Get-SubscriptionTask $paths.task
        if (-not $task -or $task.xml -cne $pending.task_before) { throw 'Subscription task changed before deferred restart.' }
        Start-SubscriptionTask $paths.task
        $state = Read-CodeToolsJson $paths.state
        Assert-SubscriptionState $state $paths
        Wait-SubscriptionReady $paths $state.port
    }
    Complete-HarnessSubscriptionRouting $CodexHome
}
function Invoke-HarnessSubscriptionRouting {
    [CmdletBinding()]
    param([string]$SourceRoot,[string]$UserHome,[string]$CodexHome,[string]$CodexCommand,[string]$DependencyUserHome,
        [ValidateSet('Install','Update','Check','Disconnect','Recover')][string]$Mode,[switch]$Preview,[switch]$DeferCommit,[scriptblock]$Checkpoint)
    if ($Mode -eq 'Recover') { return Restore-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome -CodexCommand $CodexCommand -Preview:$Preview }
    $paths = Get-SubscriptionPaths $SourceRoot $UserHome $CodexHome
    $state = Read-CodeToolsJson $paths.state
    if ($state) { Assert-SubscriptionState $state $paths }
    if (Read-CodeToolsJson $paths.pending) { throw 'An interrupted subscription operation needs Recover.' }
    $task = Get-SubscriptionTask $paths.task
    if ($task -and (-not $state -or $task.xml -cne $state.task_xml)) { throw 'Foreign or changed subscription task preserved.' }
    $links = @{}
    foreach ($name in @('configLink','roleLink')) {
        $links[$name] = Get-SubscriptionLink $paths[$name]
        if ($links[$name] -and (-not $state -or $links[$name] -ne $state.links[$name])) { throw 'Foreign subscription source link preserved.' }
    }
    $dependency = Get-SubscriptionDependency $paths $CodexCommand
    $config = Read-CodeToolsJson $paths.configSource
    if (-not $config -or $config.hostname -ne '127.0.0.1' -or $config.port -lt 1024 -or $config.port -gt 65535 -or $config.codexAutoStart -ne $false -or $config.codexShimAutoRestore -ne $false) { throw 'Subscription source must select loopback and preserve the ordinary Codex launcher.' }
    if ($Preview) { return @{ status = 'preview-subscriptions'; mode = $Mode; dependency = if ($dependency) { 'reused' } else { 'install-private-pinned' }; task = $paths.task; port = $config.port } }
    if ($Mode -eq 'Check') {
        $ready = $state -and $dependency -and $task -and $task.running -and $links.configLink -eq $paths.configSource -and $links.roleLink -eq $paths.roleSource -and (Test-SubscriptionReady $paths $config.port)
        return @{ status = if ($ready) { 'ready' } elseif ($state) { 'degraded' } else { 'disconnected' }; task = $paths.task; port = $config.port; dependency = [bool]$dependency }
    }
    if ($Mode -eq 'Disconnect' -and -not $state) { return @{ status = 'disconnected' } }
    if ($Mode -in @('Install','Update') -and -not (Test-Path -LiteralPath $paths.roleSource -PathType Container)) { throw 'Subscription role source is absent.' }
    $pending = @{ schema_version = 1; owner = 'codex-harness-subscriptions'; source = $paths.source; user = $paths.user; codex = $paths.codex; task = $paths.task
        state_before = Get-SubscriptionSnapshot $paths.state; state_after = Get-SubscriptionSnapshot $paths.state
        service_before = Get-SubscriptionSnapshot $paths.service; service_after = Get-SubscriptionSnapshot $paths.service
        config_before = Get-SubscriptionSnapshot $paths.config; config_after = Get-SubscriptionSnapshot $paths.config; config_native = Get-SubscriptionSnapshot $paths.config
        task_before = if ($task) { $task.xml } else { $null }; task_after = if ($task) { $task.xml } else { $null }; running_before = [bool]($task -and $task.running)
        links_before = $links; links_after = $links.Clone(); dependency = $dependency; runtime_started = $false; package_attempted = $false }
    Write-CodeToolsJson $paths.pending $pending
    try {
        if ($task) { $pending.task_after = $null; Write-CodeToolsJson $paths.pending $pending; Set-SubscriptionTask $paths.task $null $task.xml $paths }
        if ($state) { Restore-SubscriptionNative $paths $state.dependency; $pending.config_after = Get-SubscriptionSnapshot $paths.config; $pending.config_native = $pending.config_after; Write-CodeToolsJson $paths.pending $pending }
        if ($Mode -eq 'Disconnect') {
            foreach ($name in @('roleLink','configLink')) { $pending.links_after[$name] = $null; Write-CodeToolsJson $paths.pending $pending; Set-SubscriptionLink $paths[$name] $null $links[$name] }
            $pending.state_after = $null; $pending.service_after = $null; Write-CodeToolsJson $paths.pending $pending
            Remove-CodeToolsFile $paths.state; Remove-CodeToolsFile $paths.service
        } else {
            if (-not $dependency) {
                $pending.package_attempted = $true; Write-CodeToolsJson $paths.pending $pending
                $dependency = Initialize-SubscriptionDependency $paths $CodexCommand
                # A verified package is a reusable dependency cache, retained on
                # Disconnect/rollback just like an existing global installation.
                $pending.dependency = $dependency; $pending.package_attempted = $false; $pending.package_acquired = $true
                Write-CodeToolsJson $paths.pending $pending
            }
            Assert-SubscriptionConfiguration $paths $dependency
            $pending.links_after.configLink = $paths.configSource; Write-CodeToolsJson $paths.pending $pending
            Set-SubscriptionLink $paths.configLink $paths.configSource $links.configLink
            $descriptor = @{ schema_version = 1; owner = 'codex-harness-subscriptions'; source = $paths.source; user = $paths.user; codex = $paths.codex; task = $paths.task; port = $config.port; dependency = $dependency }
            $pending.service_after = [Convert]::ToBase64String([Text.UTF8Encoding]::new($false).GetBytes(($descriptor | ConvertTo-Json -Depth 60)))
            Write-CodeToolsJson $paths.pending $pending; Write-CodeToolsBytes $paths.service ([Convert]::FromBase64String($pending.service_after))
            $xml = New-SubscriptionTaskXml $paths $dependency.powershell
            $pending.task_after = $xml; Write-CodeToolsJson $paths.pending $pending
            Set-SubscriptionTask $paths.task $xml $null $paths
            $pending.task_after = (Get-SubscriptionTask $paths.task).xml; Write-CodeToolsJson $paths.pending $pending
            $pending.runtime_started = $true; Write-CodeToolsJson $paths.pending $pending
            Start-SubscriptionTask $paths.task; Wait-SubscriptionReady $paths $config.port
            $pending.config_after = Get-SubscriptionSnapshot $paths.config; Write-CodeToolsJson $paths.pending $pending
            $pending.links_after.roleLink = $paths.roleSource; Write-CodeToolsJson $paths.pending $pending
            Set-SubscriptionLink $paths.roleLink $paths.roleSource $links.roleLink
            $descriptor.task_xml = $pending.task_after; $descriptor.links = $pending.links_after
            $pending.state_after = [Convert]::ToBase64String([Text.UTF8Encoding]::new($false).GetBytes(($descriptor | ConvertTo-Json -Depth 60)))
            Write-CodeToolsJson $paths.pending $pending; Write-CodeToolsBytes $paths.state ([Convert]::FromBase64String($pending.state_after))
        }
        if ($Checkpoint) { & $Checkpoint 'subscriptions' }
        if (-not $DeferCommit) { Complete-HarnessSubscriptionRouting $CodexHome }
        @{ status = if ($Mode -eq 'Disconnect') { 'disconnected' } else { 'ready' }; task = $paths.task; port = $config.port }
    } catch {
        $cause = $_.Exception.Message
        # The outer coordinator restores MCP/core after routing. Do not restart
        # a config writer while those exact-byte rollback checks are pending.
        if ($DeferCommit) { throw "Subscription activation failed; coordinator recovery required: $cause" }
        try { Restore-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome -CodexCommand $CodexCommand | Out-Null }
        catch { throw "Subscription activation failed: $cause. Recovery pending: $($_.Exception.Message)" }
        throw "Subscription activation failed and prior state restored: $cause"
    }
}
Export-ModuleMember -Function Invoke-HarnessSubscriptionRouting, Restore-HarnessSubscriptionRouting, Resume-HarnessSubscriptionRouting, Complete-HarnessSubscriptionRouting, Invoke-SubscriptionServiceHost
