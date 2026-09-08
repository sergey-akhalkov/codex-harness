#requires -Version 7.4
# Owned native artifacts and registrations; source configuration remains linked.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-TokenWorkflowBuild {
    param([string]$SourceRoot, [string]$CodexHome)
    $root = Join-Path $CodexHome 'harness/rtk'
    $definition = Get-Content -LiteralPath (Join-Path $SourceRoot 'global/rtk.json') -Raw | ConvertFrom-Json -AsHashtable
    if (-not $IsWindows -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture -ne 'X64') { throw 'RTK selection currently requires Windows x64.' }
    $sourceFiles = @('Cargo.toml','Cargo.lock','tools/rtk-adapter/Cargo.toml','tools/rtk-adapter/src/main.rs')
    $hashes = @(foreach ($relative in $sourceFiles) { (Get-FileHash -LiteralPath (Join-Path $SourceRoot $relative) -Algorithm SHA256).Hash })
    $hashes += (Get-FileHash -LiteralPath (Join-Path $SourceRoot 'global/rtk.json')).Hash
    $hashes += (Get-FileHash -LiteralPath (Join-Path $SourceRoot 'tools/token-workflow.psm1')).Hash
    $identity = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes(($hashes -join ':')))).ToLowerInvariant()
    $build = Join-Path $root "build/$identity"
    $adapter = Join-Path $build 'harness-rtk.exe'
    $package = Join-Path $root ('packages/' + $definition.version)
    $rtk = Join-Path $package 'rtk.exe'
    $core = Get-Module kit
    & $core { param($p,$h) Assert-HarnessWithin $p (Join-Path $h 'harness'); Assert-HarnessOrdinaryParents (Join-Path $p 'probe') } $root $CodexHome
    foreach($path in @($rtk,$adapter,(Join-Path $build 'build.json'),(Join-Path $build 'cargo.log'),(Join-Path $root 'cargo-target/probe'))){
        & $core { param($p) Assert-HarnessOrdinaryParents $p } $path
        $item=Get-Item -LiteralPath $path -Force -ErrorAction SilentlyContinue
        if($item -and ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint))){throw "Unexpected RTK artifact path; preserving $path"}
    }
    if (-not (Test-Path -LiteralPath $rtk)) {
        [void][IO.Directory]::CreateDirectory($package)
        $archive = Join-Path $package ('download-' + [guid]::NewGuid().ToString('N') + '.zip')
        try {
            Invoke-WebRequest $definition.url -OutFile $archive -TimeoutSec 60
            if ((Get-FileHash -LiteralPath $archive).Hash -ine $definition.sha256) { throw 'RTK release checksum mismatch; dependency not installed.' }
            # Pinned archive checksum is checked before extraction into the owned package root.
            Expand-Archive -LiteralPath $archive -DestinationPath $package
        } finally { if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive } }
    }
    if ((Get-FileHash -LiteralPath $rtk).Hash -ine $definition.executableSha256) { throw 'RTK binary identity mismatch; preserving the unexpected file.' }
    $buildRecord = Join-Path $build 'build.json'
    if (-not (Test-Path -LiteralPath $adapter)) {
        [void][IO.Directory]::CreateDirectory($build)
        $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
        $target = Join-Path $root 'cargo-target'
        $log = Join-Path $build 'cargo.log'
        $start=[Diagnostics.ProcessStartInfo]::new($cargo)
        $start.UseShellExecute=$false;$start.CreateNoWindow=$true
        $start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
        $start.WorkingDirectory=$SourceRoot
        $start.Environment['CARGO_TARGET_DIR']=$target
        foreach($argument in @('build','--release','--locked','-p','harness-rtk','--manifest-path',(Join-Path $SourceRoot 'Cargo.toml'))){$start.ArgumentList.Add($argument)}
        $process=[Diagnostics.Process]::Start($start)
        try {
            $out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
            $finished=$process.WaitForExit(600000)
            if(-not $finished){$process.Kill($true);$process.WaitForExit()}
            [IO.File]::WriteAllText($log,($out.GetAwaiter().GetResult()+$err.GetAwaiter().GetResult()))
            if(-not $finished){throw "RTK adapter build timed out; see $log"}
            if ($process.ExitCode) { throw "RTK adapter build failed; see $log" }
            Copy-Item -LiteralPath (Join-Path $target 'release/harness-rtk.exe') -Destination $adapter
            & $core { param($p,$value) Write-HarnessJson $p $value } $buildRecord @{sourceIdentity=$identity;sourceHashes=$hashes;binarySha256=(Get-FileHash -LiteralPath $adapter).Hash;cargo=(& $cargo --version)}
        } finally { $process.Dispose() }
    }
    if (-not (Test-Path -LiteralPath $buildRecord)) { throw 'Unidentified RTK adapter build; preserving it for inspection.' }
    $record = Get-Content -LiteralPath $buildRecord -Raw | ConvertFrom-Json -AsHashtable
    if ($record.sourceIdentity -ne $identity -or (Get-FileHash -LiteralPath $adapter).Hash -ine $record.binarySha256) { throw 'RTK adapter build identity mismatch.' }
    # current_exe can resolve through the public link to the immutable build.
    # Keep its dependency available in either invocation location.
    $sibling=Join-Path $build 'rtk.exe'
    if(Get-Item -LiteralPath $sibling -Force -ErrorAction SilentlyContinue){
        $target=& $core { param($p) Get-HarnessLinkTarget $p } $sibling
        if($target -ne $rtk){throw 'RTK build dependency link conflict; preserving it.'}
    } else { New-Item -ItemType SymbolicLink -Path $sibling -Target $rtk | Out-Null }
    @{adapter=$adapter;rtk=$rtk;sourceIdentity=$identity;version=$definition.version;binarySha256=$record.binarySha256}
}

function Invoke-HarnessTokenWorkflow {
    [CmdletBinding()]
    param([string]$SourceRoot, [string]$CodexHome, [string]$UserHome, [string]$CodexCommand,
        [ValidateSet('Install','Update','Check','Disconnect','Recover')][string]$Mode='Install', [switch]$Preview)
    $SourceRoot = [IO.Path]::GetFullPath($SourceRoot)
    $CodexHome = [IO.Path]::GetFullPath($CodexHome)
    $core = Get-Module kit
    if (-not $core) { $core = Import-Module (Join-Path $SourceRoot 'tools/kit.psm1') -PassThru }
    $statePath = Join-Path $CodexHome 'harness/token-workflow.json'
    $pendingPath = Join-Path $CodexHome 'harness/token-workflow-pending.json'
    & $core { param($p) Assert-HarnessOrdinaryParents $p } $statePath
    $state = & $core { param($p) Read-HarnessJson $p } $statePath
    $installation = & $core { param($p) Read-HarnessJson $p } (Join-Path $CodexHome 'harness/installation.json')
    if (-not $CodexCommand -and $installation) { $CodexCommand = $installation.codexCommand }
    $dependencyOwner = if ($installation -and $installation.ContainsKey('dependencyUserHome')) { $installation.dependencyUserHome } else { $UserHome }
    $pending = & $core { param($p) Read-HarnessJson $p } $pendingPath
    if ($pending -and $Mode -ne 'Recover') { throw 'Token workflow transaction is pending; use -TokenWorkflowOnly -Mode Recover.' }
    if ($Mode -eq 'Recover') {
        if (-not $pending) { return @{status='No token workflow transaction'} }
        if ($Preview) { return @{status='Preview token workflow recovery'} }
        if(Test-Path -LiteralPath (Join-Path $CodexHome 'harness/pending.json')) {
            Invoke-HarnessInstall -SourceRoot $SourceRoot -CodexHome $CodexHome -UserHome $UserHome -DependencyUserHome $dependencyOwner -CodexCommand $CodexCommand -Mode Recover | Out-Null
        }
        # Roll back only registrations whose current target is one of this transaction's targets.
        foreach ($op in @($pending.operations)) {
            & $core { param($p,$h) Assert-HarnessWithin $p (Join-Path $h 'harness/bin'); Assert-HarnessOrdinaryParents $p } $op.destination $CodexHome
            $target = & $core { param($p) Get-HarnessLinkTarget $p } $op.destination
            if ($target -and $target -notin @($op.oldSource,$op.newSource)) { throw "Token workflow recovery conflict: $($op.destination)" }
        }
        & $core { param($ops) Undo-HarnessOperations $ops } @($pending.operations)
        if ($pending.previousState) { & $core { param($p,$s) Write-HarnessJson $p $s } $statePath $pending.previousState }
        elseif (Test-Path -LiteralPath $statePath) { Remove-Item -LiteralPath $statePath }
        # Reconcile live source links from the recovered selection. A failed/new
        # activation returns to suspension, never resurrecting old diagnostics.
        if (-not ($pending.previousState -and $pending.previousState.enabled)) { Set-HarnessNativeFeature $CodexHome $CodexCommand 'hooks' $false }
        if ($installation) { Invoke-HarnessInstall -SourceRoot $SourceRoot -CodexHome $CodexHome -UserHome $UserHome -DependencyUserHome $dependencyOwner -CodexCommand $CodexCommand -IncludeCodeTools | Out-Null }
        Remove-Item -LiteralPath $pendingPath
        return @{status='Recovered token workflow'}
    }
    if ($Mode -eq 'Check') {
        if (-not $state -or -not $state.enabled) { return @{status='Token workflow disconnected'} }
        foreach ($op in $state.links) {
            $target = & $core { param($p) Get-HarnessLinkTarget $p } $op.destination
            if ($target -ne $op.source -or -not (Test-Path -LiteralPath $op.source) -or (Get-FileHash -LiteralPath $op.source).Hash -ine $op.sha256) { throw "Token workflow artifact mismatch: $($op.destination)" }
        }
        return @{status='Token workflow connected';rtkVersion=$state.rtkVersion;sourceIdentity=$state.sourceIdentity;note='Native features and exact hook trust are checked by the consumer probe.'}
    }
    if ($Mode -eq 'Disconnect' -and (-not $state -or -not $state.enabled)) { return @{status='Token workflow disconnected'} }
    if (-not $installation) { throw 'Install the linked core before connecting the token workflow.' }
    if ($Preview) { return @{status="Preview token workflow $Mode";effect='Owned binaries/source links, native RTK hook selection and Code Mode; no model calls.'} }
    $build = if ($Mode -ne 'Disconnect') { Get-TokenWorkflowBuild $SourceRoot $CodexHome } else { $null }
    $operations = [Collections.Generic.List[object]]::new()
    $links = [Collections.Generic.List[object]]::new()
    foreach ($name in @('harness-rtk.exe','rtk.exe')) {
        $destination = Join-Path $CodexHome "harness/bin/$name"
        & $core { param($p) Assert-HarnessOrdinaryParents $p } $destination
        $current = Get-Item -LiteralPath $destination -Force -ErrorAction SilentlyContinue
        $target = & $core { param($p) Get-HarnessLinkTarget $p } $destination
        $old = if ($state) { @($state.links | Where-Object destination -eq $destination) | Select-Object -First 1 } else { $null }
        if ($current -and (-not $old -or $target -ne $old.source)) { throw "Token workflow target conflict; preserving $destination" }
        $next = if ($build) { if ($name -eq 'rtk.exe') { $build.rtk } else { $build.adapter } } else { $null }
        if ($target -ne $next) { $operations.Add(@{destination=$destination;oldSource=$target;newSource=$next}) }
        if ($next) { $links.Add(@{destination=$destination;source=$next;sha256=(Get-FileHash -LiteralPath $next).Hash}) }
    }
    $nextState = @{schemaVersion=1;enabled=($Mode -ne 'Disconnect');links=@($links);sourceRoot=$SourceRoot}
    $features = Get-HarnessNativeFeatures $CodexHome $CodexCommand
    $nextState.previousCodeMode = if ($state -and $state.enabled) { $state.previousCodeMode } else { $features.code_mode }
    if ($build) { $nextState.sourceIdentity=$build.sourceIdentity; $nextState.rtkVersion=$build.version }
    & $core { param($p,$s) Write-HarnessJson $p $s } $pendingPath @{previousState=$state;plannedState=$nextState;operations=@($operations)}
    try {
        foreach ($op in $operations) {
            if ($op.oldSource) { & $core { param($p,$s) Remove-HarnessLink $p $s } $op.destination $op.oldSource }
            if ($op.newSource) { New-Item -ItemType SymbolicLink -Path $op.destination -Target $op.newSource | Out-Null }
        }
        & $core { param($p,$s) Write-HarnessJson $p $s } $statePath $nextState
        Invoke-HarnessInstall -SourceRoot $SourceRoot -CodexHome $CodexHome -UserHome $UserHome -DependencyUserHome $dependencyOwner -CodexCommand $CodexCommand -IncludeCodeTools | Out-Null
        if ($Mode -eq 'Disconnect') {
            Set-HarnessNativeFeature $CodexHome $CodexCommand 'hooks' $false
            if ($features.code_mode) { Set-HarnessNativeFeature $CodexHome $CodexCommand 'code_mode' ([bool]$state.previousCodeMode) }
        } elseif (-not $state -or -not $state.enabled) {
            Set-HarnessNativeFeature $CodexHome $CodexCommand 'code_mode' $true
            Set-HarnessNativeFeature $CodexHome $CodexCommand 'hooks' $true
        }
        Remove-Item -LiteralPath $pendingPath
        @{status= $(if($nextState.enabled){'Token workflow connected'}else{'Token workflow disconnected'});rtkVersion= $(if($build){$build.version}else{$null})}
    } catch {
        throw "Token workflow activation incomplete: $($_.Exception.Message). Owned transaction retained; use -TokenWorkflowOnly -Mode Recover."
    }
}

Export-ModuleMember -Function Invoke-HarnessTokenWorkflow
