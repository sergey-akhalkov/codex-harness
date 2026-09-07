#requires -Version 7.4
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSScriptRoot 'kit.psm1')
Import-Module (Join-Path $PSScriptRoot 'code-tools.psm1')
Import-Module (Join-Path $PSScriptRoot 'subscription-routing.psm1')

function Get-BootstrapIdentity([string]$Directory) {
    Assert-CodeToolsPlain $Directory
    $root = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    $pending = [Collections.Generic.Stack[IO.DirectoryInfo]]::new()
    $pending.Push([IO.DirectoryInfo]::new($root))
    $lines = [Collections.Generic.List[string]]::new()
    while ($pending.Count) {
        foreach ($item in $pending.Pop().EnumerateFileSystemInfos()) {
            $full = [IO.Path]::GetFullPath($item.FullName)
            if (-not $full.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Bootstrap identity escaped its owned directory.' }
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Bootstrap package contains a reparse point; preserving: $full" }
            if ($item -is [IO.DirectoryInfo]) { $pending.Push($item); continue }
            # UV's cold wheel build can generate base-interpreter bytecode after
            # installation. Derived caches are not source/package modifications.
            if ($item.Extension -in @('.pyc','.pyo')) { continue }
            $stream = [IO.File]::OpenRead($full)
            try { $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)) } finally { $stream.Dispose() }
            $lines.Add($full.Substring($root.Length) + ':' + $hash)
        }
    }
    $lines.Sort([StringComparer]::Ordinal)
    Get-CodeToolsHash ([Text.Encoding]::UTF8.GetBytes(($lines -join '|')))
}

function Assert-BootstrapIdle([string]$Path) {
    $full = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    $consumers = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
        ($_.ExecutablePath -and ($_.ExecutablePath -eq $full -or $_.ExecutablePath.StartsWith($full + '\', [StringComparison]::OrdinalIgnoreCase))) -or
        ($_.CommandLine -and $_.CommandLine.Contains($full, [StringComparison]::OrdinalIgnoreCase))
    })
    if ($consumers.Count) { throw "An active consumer uses bootstrap dependency; preserving: $full" }
}

function Get-BootstrapUvRelease([string]$AssetName) {
    try { return Invoke-RestMethod 'https://api.github.com/repos/astral-sh/uv/releases/latest' -TimeoutSec 30 }
    catch {
        # GitHub's anonymous REST quota is shared by the public egress IP. Its
        # official release redirect and published checksum require no REST API.
        $response = Invoke-WebRequest 'https://github.com/astral-sh/uv/releases/latest' -Method Head -TimeoutSec 30
        $uri = $response.BaseResponse.RequestMessage.RequestUri
        if ($uri.Host -ne 'github.com' -or $uri.AbsolutePath -notmatch '^/astral-sh/uv/releases/tag/([0-9]+\.[0-9]+\.[0-9]+)$') { throw 'Cannot establish a stable official uv release from the release redirect.' }
        $version = $Matches[1]
        $assetUrl = 'https://github.com/astral-sh/uv/releases/download/' + $version + '/' + $AssetName
        $checksum = (Invoke-WebRequest ($assetUrl + '.sha256') -TimeoutSec 30).Content
        if ($checksum -is [byte[]]) { $checksum = [Text.Encoding]::UTF8.GetString($checksum) }
        if ([string]$checksum -notmatch '^([a-fA-F0-9]{64})(?:\s|$)') { throw 'Official uv checksum file has an unexpected format.' }
        @{ tag_name = $version; assets = @(@{ name = $AssetName; digest = 'sha256:' + $Matches[1].ToLowerInvariant(); browser_download_url = $assetUrl }) }
    }
}

function Initialize-BootstrapBase([string]$UserHome, [string]$CodexHome, [string]$CodexCommand) {
    $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
    if ($runtime.uv -and $runtime.lifecycle_python) { return $runtime }
    $pendingPath = Join-Path $CodexHome 'harness/bootstrap-runtime-pending.json'
    if (Test-Path -LiteralPath $pendingPath) { throw 'An interrupted base-runtime bootstrap needs Recover.' }
    $journal = @{ schema_version = 1; uv_files = @(); python = $null }
    Write-CodeToolsJson $pendingPath $journal
    if (-not $runtime.uv) {
        # Official release metadata supplies the archive digest. No installer
        # script is executed and no PATH/registry/shell profile is rewritten.
        $architecture = switch ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) { 'X64' { 'x86_64' } 'Arm64' { 'aarch64' } default { throw 'uv bootstrap supports Windows x64/ARM64.' } }
        $assetName = 'uv-' + $architecture + '-pc-windows-msvc.zip'
        $release = Get-BootstrapUvRelease $assetName
        $assets = @($release.assets | Where-Object name -EQ $assetName)
        if ($assets.Count -ne 1 -or $assets[0].digest -notmatch '^sha256:[a-f0-9]{64}$') { throw 'Official uv release lacks an unambiguous SHA256 archive digest.' }
        $asset = $assets[0]
        if ($asset.browser_download_url -notlike 'https://github.com/astral-sh/uv/releases/download/*') { throw 'Unexpected uv release origin.' }
        $archivePath = Join-Path $CodexHome ('harness/dependencies/bootstrap/' + [guid]::NewGuid().ToString('N') + '/uv.zip')
        Assert-CodeToolsPlain $archivePath
        [IO.Directory]::CreateDirectory((Split-Path $archivePath)) | Out-Null
        Invoke-WebRequest $asset.browser_download_url -OutFile $archivePath -TimeoutSec 180
        if ((Get-CodeToolsHash (Get-CodeToolsBytes $archivePath)) -ne $asset.digest.Substring(7)) { throw 'Official uv archive checksum mismatch; no executable was installed.' }
        $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
        try {
            foreach ($name in @('uv.exe','uvx.exe')) {
                $entries = @($archive.Entries | Where-Object FullName -EQ $name)
                if ($entries.Count -ne 1) { throw "Unexpected uv archive executable layout: $name" }
                $target = Join-Path $UserHome ('.local/bin/' + $name)
                Assert-CodeToolsPlain $target
                if (Test-Path -LiteralPath $target) { throw "An existing uv file is preserved: $target" }
                $stream = $entries[0].Open(); $memory = [IO.MemoryStream]::new()
                try { $stream.CopyTo($memory); $bytes = $memory.ToArray() } finally { $stream.Dispose(); $memory.Dispose() }
                $journal.uv_files += @{ name = $name; sha256 = Get-CodeToolsHash $bytes; source = $asset.browser_download_url; archive_sha256 = $asset.digest.Substring(7); version = $release.tag_name }
                Write-CodeToolsJson $pendingPath $journal
                Write-CodeToolsBytes $target $bytes
            }
        } finally { $archive.Dispose() }
        $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
        $version = & $runtime.uv --version
        if ($LASTEXITCODE -ne 0 -or $version -notmatch ('^uv ' + [regex]::Escape($release.tag_name) + '\b')) { throw 'Installed official uv version verification failed.' }
    }
    if (-not $runtime.lifecycle_python) {
        # uv selects its published CPython3.13 build and verifies the distribution
        # checksum. --no-bin/--no-registry leave unrelated Python selection intact.
        $raw = & $runtime.uv python list 3.13 --only-downloads --output-format json --no-config
        if ($LASTEXITCODE -ne 0) { throw 'uv could not enumerate its declared managed Python distribution.' }
        $distribution = @(($raw -join [Environment]::NewLine | ConvertFrom-Json) | Where-Object { $_.implementation -eq 'cpython' -and $_.variant -eq 'default' })[0]
        $target = Join-Path $runtime.python_root $distribution.key
        Assert-CodeToolsPlain $target
        if (Test-Path -LiteralPath $target) { throw 'An existing incomplete Python installation is preserved.' }
        $journal.python = @{ key = $distribution.key; target = $target; root = $runtime.python_root; phase = 'prepared'; identity = $null; source = $distribution.url }
        Write-CodeToolsJson $pendingPath $journal
        & $runtime.uv python install $distribution.key --install-dir $runtime.python_root --no-bin --no-registry --no-config --no-progress | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Explicit managed Python installation failed; its ownership journal is retained.' }
        $journal.python.identity = Get-BootstrapIdentity $target
        $journal.python.phase = 'installed'
        Write-CodeToolsJson $pendingPath $journal
    }
    Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
}

function Restore-BootstrapBase([string]$UserHome, [string]$CodexHome, [string]$CodexCommand, [switch]$Preview) {
    $pendingPath = Join-Path $CodexHome 'harness/bootstrap-runtime-pending.json'
    $journal = Read-CodeToolsJson $pendingPath
    if (-not $journal) { return }
    if ($journal.schema_version -ne 1) { throw 'Unknown base-runtime bootstrap recovery schema.' }
    $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
    if ($journal.python) {
        $record = $journal.python
        if ($record.key -notmatch '^cpython-3\.13\.[0-9]+-windows-(?:x86_64|aarch64)-none$' -or
            [IO.Path]::GetFullPath($record.target) -ne [IO.Path]::GetFullPath((Join-Path $runtime.python_root $record.key))) { throw 'Bootstrap Python recovery ownership mismatch.' }
        Assert-CodeToolsPlain $record.target
        if (Test-Path -LiteralPath $record.target) {
            if ($record.phase -ne 'installed' -or (Get-BootstrapIdentity $record.target) -ne $record.identity) { throw 'Bootstrap Python is incomplete or changed; preserving its pending record.' }
            # A UV tool environment may depend on this base even while idle.
            $dependents = @(Get-ChildItem -LiteralPath $runtime.uv_root -Filter pyvenv.cfg -File -Recurse -ErrorAction SilentlyContinue | Where-Object { (Get-Content -LiteralPath $_.FullName -Raw).Contains($record.target, [StringComparison]::OrdinalIgnoreCase) })
            if ($dependents.Count) { throw 'An installed uv tool still uses bootstrap Python; preserving it.' }
            Assert-BootstrapIdle $record.target
            if (-not $Preview) {
                if (-not $runtime.uv) { throw 'Existing uv is needed to uninstall the owned bootstrap Python.' }
                & $runtime.uv python uninstall $record.key --install-dir $runtime.python_root --no-config | Out-Host
                if ($LASTEXITCODE -ne 0) { throw 'Bootstrap Python rollback remains pending.' }
            }
        }
    }
    foreach ($record in $journal.uv_files) {
        if ($record.name -notin @('uv.exe','uvx.exe')) { throw 'Unowned uv bootstrap destination.' }
        $target = Join-Path $UserHome ('.local/bin/' + $record.name)
        if (Test-Path -LiteralPath $target) {
            if ((Get-CodeToolsHash (Get-CodeToolsBytes $target)) -ne $record.sha256) { throw 'Bootstrapped uv executable changed; preserving it.' }
            Assert-BootstrapIdle $target
        }
    }
    if (-not $Preview) {
        foreach ($record in $journal.uv_files) { Remove-CodeToolsFile (Join-Path $UserHome ('.local/bin/' + $record.name)) }
        Remove-CodeToolsFile $pendingPath
    }
}

function Invoke-BootstrapUv($Runtime, [string[]]$Arguments) {
    $oldRoot = $env:UV_TOOL_DIR; $oldBin = $env:UV_TOOL_BIN_DIR
    try {
        $env:UV_TOOL_DIR = $Runtime.uv_root; $env:UV_TOOL_BIN_DIR = $Runtime.uv_bin
        & $Runtime.uv @Arguments --no-config | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Explicit Serena bootstrap failed (uv exit $LASTEXITCODE); its pending journal is preserved." }
    } finally { $env:UV_TOOL_DIR = $oldRoot; $env:UV_TOOL_BIN_DIR = $oldBin }
}

function Initialize-CodeToolsRuntime([string]$UserHome, [string]$CodexHome, [string]$CodexCommand, [ValidateSet('serena','graphify')][string]$Tool = 'serena') {
    $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
    $definition = if ($Tool -eq 'serena') { @{ package = 'serena-agent'; requirement = 'serena-agent==1.7.0'; wrappers = @('serena.exe','serena-agent.exe','serena-hooks.exe'); journal = 'bootstrap-pending.json'; installed = $runtime.python } } else { @{ package = 'graphifyy'; requirement = 'graphifyy[mcp]==0.9.55'; wrappers = @('graphify.exe','graphify-mcp.exe'); journal = 'bootstrap-graphify-pending.json'; installed = $runtime.graphify_python } }
    if ($definition.installed) { return $runtime }
    if (-not $runtime.uv -or -not $runtime.lifecycle_python) { $runtime = Initialize-BootstrapBase $UserHome $CodexHome $CodexCommand }
    $target = Join-Path $runtime.uv_root $definition.package
    Assert-CodeToolsPlain $target
    if (Test-Path -LiteralPath $target) { throw "An incomplete $Tool environment exists; preserve it and resolve package ownership before bootstrap." }
    $wrappers = @($definition.wrappers | ForEach-Object { Join-Path $runtime.uv_bin $_ })
    foreach ($wrapper in $wrappers) {
        Assert-CodeToolsPlain $wrapper
        if (Test-Path -LiteralPath $wrapper) { throw "Bootstrap entry point is already owned; preserving: $wrapper" }
    }
    $path = Join-Path $CodexHome ('harness/' + $definition.journal)
    if (Test-Path -LiteralPath $path) { throw 'An interrupted bootstrap needs Recover.' }
    $record = @{ schema_version = 1; phase = 'prepared'; target = $target; uv_root = $runtime.uv_root; uv_bin = $runtime.uv_bin; wrappers = $wrappers; identity = $null; wrapper_hashes = @{} }
    Write-CodeToolsJson $path $record
    # Wrapper compatibility fixes the current stable requirement; existing tools are reused.
    Invoke-BootstrapUv $runtime @('tool','install',$definition.requirement,'--python',$runtime.lifecycle_python,'--no-python-downloads','--no-progress')
    $record.identity = Get-BootstrapIdentity $target
    foreach ($wrapper in $wrappers) { $record.wrapper_hashes[$wrapper] = Get-CodeToolsHash (Get-CodeToolsBytes $wrapper) }
    $record.phase = 'installed'
    Write-CodeToolsJson $path $record
    Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
}

function Restore-CodeToolsBootstrap([string]$UserHome, [string]$CodexHome, [string]$CodexCommand, [switch]$Preview, [ValidateSet('serena','graphify')][string]$Tool = 'serena') {
    $definition = if ($Tool -eq 'serena') { @{ package = 'serena-agent'; wrappers = @('serena.exe','serena-agent.exe','serena-hooks.exe'); journal = 'bootstrap-pending.json' } } else { @{ package = 'graphifyy'; wrappers = @('graphify.exe','graphify-mcp.exe'); journal = 'bootstrap-graphify-pending.json' } }
    $path = Join-Path $CodexHome ('harness/' + $definition.journal)
    $record = Read-CodeToolsJson $path
    if (-not $record) { return }
    $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
    $expected = [IO.Path]::GetFullPath((Join-Path $runtime.uv_root $definition.package))
    if ($record.schema_version -ne 1 -or $expected -ne [IO.Path]::GetFullPath($record.target) -or $record.uv_bin -ne $runtime.uv_bin) { throw 'Bootstrap journal ownership changed; preserving it.' }
    Assert-CodeToolsPlain $expected
    $wrappers = @($definition.wrappers | ForEach-Object { Join-Path $runtime.uv_bin $_ })
    if (@(Compare-Object $wrappers $record.wrappers).Count) { throw 'Bootstrap wrapper ownership changed.' }
    $present = (Test-Path -LiteralPath $expected) -or @($wrappers | Where-Object { Test-Path -LiteralPath $_ }).Count
    if ($present) {
        if ($record.phase -ne 'installed') { throw 'Bootstrap was interrupted before ownership verification. Shared package artifacts are preserved; inspect bootstrap-pending.json before recovery.' }
        if (-not $runtime.uv) { throw 'The existing uv executable is unavailable; bootstrap package rollback remains pending.' }
        if ((Get-BootstrapIdentity $expected) -ne $record.identity) { throw 'Bootstrap environment changed after installation; preserving it.' }
        foreach ($wrapper in $wrappers) {
            if ((Get-CodeToolsHash (Get-CodeToolsBytes $wrapper)) -ne $record.wrapper_hashes[$wrapper]) { throw "Bootstrap wrapper changed; preserving: $wrapper" }
        }
        $consumers = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
            ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($expected + '\', [StringComparison]::OrdinalIgnoreCase)) -or
            ($_.CommandLine -and $_.CommandLine.Contains($expected, [StringComparison]::OrdinalIgnoreCase))
        })
        if ($consumers.Count) { throw 'An active consumer uses the newly bootstrapped Serena environment; package rollback remains pending.' }
        if (-not $Preview) { Invoke-BootstrapUv $runtime @('tool','uninstall',$definition.package) }
    }
    if (-not $Preview) { Remove-CodeToolsFile $path }
}

function Complete-HarnessActivation([string]$CodexHome) {
    # The outer durable committed marker is written BEFORE any component journal
    # is removed. A crash here completes commit cleanup instead of undoing it.
    foreach ($name in @('pending.json','code-tools-registration-pending.json','code-tools-files-pending.json','bootstrap-pending.json','bootstrap-graphify-pending.json','bootstrap-runtime-pending.json','subscription-routing-pending.json')) {
        Remove-CodeToolsFile (Join-Path $CodexHome ('harness/' + $name))
    }
    Remove-CodeToolsFile (Join-Path $CodexHome 'harness/activation-pending.json')
}

function Restore-HarnessActivation {
    param([string]$SourceRoot, [string]$UserHome, [string]$CodexHome, [string]$CodexCommand, [string]$PathScope = 'User', [switch]$Preview, [string]$DependencyUserHome)
    $DependencyUserHome = [IO.Path]::GetFullPath($(if ($DependencyUserHome) { $DependencyUserHome } else { $UserHome }))
    $pendingPath = Join-Path $CodexHome 'harness/activation-pending.json'
    $pending = Read-CodeToolsJson $pendingPath
    if ($pending) {
        if ($pending.schema_version -ne 1 -or $pending.owner -ne 'codex-harness-activation' -or
            [IO.Path]::GetFullPath($pending.codex_home) -ne [IO.Path]::GetFullPath($CodexHome) -or
            [IO.Path]::GetFullPath($pending.user_home) -ne [IO.Path]::GetFullPath($UserHome) -or
            $pending.id -notmatch '^[a-f0-9]{32}$') { throw 'Activation recovery ownership mismatch; preserving pending state.' }
        $recordedDependencyHome = if ($pending.ContainsKey('dependency_user_home')) { $pending.dependency_user_home } else { $pending.user_home }
        if (-not [string]::Equals([IO.Path]::GetFullPath($recordedDependencyHome), $DependencyUserHome, [StringComparison]::OrdinalIgnoreCase)) { throw 'Dependency owner differs from the pending activation; preserving its state.' }
        if ($pending.phase -eq 'committed') {
            if (-not $Preview) { Complete-HarnessActivation $CodexHome }
            return @{ status = if ($Preview) { 'Preview committed cleanup' } else { 'Committed transaction cleanup completed' } }
        }
    }
    $errors = [Collections.Generic.List[string]]::new()
    # Continue independent recovery even when one component has an intervening edit.
    foreach ($operation in @(
        { Restore-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome -CodexCommand $CodexCommand -DependencyUserHome $DependencyUserHome -Preview:$Preview -DeferRestart | Out-Null },
        { Restore-CodeToolsRegistries $CodexHome -Preview:$Preview },
        { Restore-CodeToolsRegistration $CodexHome -Preview:$Preview | Out-Null },
        { Invoke-HarnessInstall -SourceRoot $SourceRoot -UserHome $UserHome -DependencyUserHome $DependencyUserHome -CodexHome $CodexHome -CodexCommand $CodexCommand -PathScope $PathScope -Mode Recover -Preview:$Preview | Out-Null }
    )) { try { & $operation } catch { $errors.Add($_.Exception.Message) } }
    if ($pending) {
        $dependencyRoot = Join-Path $CodexHome ('harness/dependencies/transactions/' + $pending.id)
        if (Test-Path -LiteralPath $dependencyRoot) {
            try {
                $runtime = Get-HarnessCodeToolsRuntime $DependencyUserHome $CodexHome $CodexCommand
                if ($Preview) { if (-not $runtime.lifecycle_python) { throw 'Dependency recovery needs an existing uv-managed Python; native config/core recovery remains available.' } }
                else {
                    $result = Invoke-CodeToolsPythonJson $runtime.lifecycle_python @((Join-Path $SourceRoot 'tools/code-tools/dependencies.py'), 'recover',
                        '--user-home', $DependencyUserHome, '--state-dir', (Join-Path $CodexHome 'harness/dependencies'), '--transaction-id', $pending.id, '--rollback-committed')
                    if (-not $result.complete) { throw "Dependency recovery remains pending: $(($result.results | Where-Object state -eq 'pending' | ForEach-Object reason) -join '; ')" }
                }
            } catch { $errors.Add($_.Exception.Message) }
        }
        $dependencyResult = Read-CodeToolsJson (Join-Path $CodexHome ('harness/dependencies/result-' + $pending.id + '.json'))
        if ($dependencyResult) {
            # Directory journals cover package promotion. Additive system-runtime
            # or model-file changes need their own inverse; never claim they vanished.
            foreach ($result in $dependencyResult.results) {
                if ($result.ContainsKey('runtime') -and $result.runtime -and $result.runtime.state -eq 'installed') {
                    $errors.Add("Additive runtime rollback is not recorded for dependency $($result.id); preserve the dependency result for explicit recovery.")
                }
                if ($result.ContainsKey('dictionary') -and $result.dictionary -and $result.dictionary.state -eq 'installed-unverified' -and -not $result.dictionary.ContainsKey('transaction_journal')) {
                    $errors.Add('New OCR dictionary has no rollback journal; preserve its lifecycle result for explicit recovery.')
                }
                if ($result.state -in @('installed','installed-unverified','updated') -and -not $result.ContainsKey('transaction_journal')) {
                    $errors.Add("Dependency $($result.id) changed without a rollback journal; preserve its explicit lifecycle result.")
                }
            }
        }
    }
    try { Restore-CodeToolsBootstrap $DependencyUserHome $CodexHome $CodexCommand -Preview:$Preview -Tool graphify } catch { $errors.Add($_.Exception.Message) }
    try { Restore-CodeToolsBootstrap $DependencyUserHome $CodexHome $CodexCommand -Preview:$Preview } catch { $errors.Add($_.Exception.Message) }
    # Base Python/uv may be removed only after the dependent Serena tool is gone.
    if (-not $errors.Count -and -not (Test-Path -LiteralPath (Join-Path $CodexHome 'harness/bootstrap-pending.json')) -and -not (Test-Path -LiteralPath (Join-Path $CodexHome 'harness/bootstrap-graphify-pending.json'))) {
        try { Restore-BootstrapBase $DependencyUserHome $CodexHome $CodexCommand -Preview:$Preview } catch { $errors.Add($_.Exception.Message) }
    }
    if ($errors.Count) {
        if ($pending -and -not $Preview) { $pending.phase = 'incompleteUpdate'; $pending.recovery_errors = $errors.ToArray(); Write-CodeToolsJson $pendingPath $pending }
        throw ('Recovery incomplete; preserved pending journal: ' + ($errors -join '; '))
    }
    if (-not $Preview) { Resume-HarnessSubscriptionRouting -SourceRoot $SourceRoot -UserHome $UserHome -CodexHome $CodexHome }
    if (-not $Preview) { Remove-CodeToolsFile $pendingPath }
    @{ status = if ($Preview) { 'Preview recovery' } else { 'Recovered' } }
}

function Invoke-HarnessActivation {
    [CmdletBinding()]
    param([string]$SourceRoot, [string]$UserHome, [string]$CodexHome, [string]$CodexCommand,
        [ValidateSet('Install','Update','Check','Disconnect','Recover')][string]$Mode,
        [ValidateSet('User','Process')][string]$PathScope = 'User', [switch]$Preview, [scriptblock]$Checkpoint, [string]$DependencyUserHome)
    $DependencyUserHome = [IO.Path]::GetFullPath($(if ($DependencyUserHome) { $DependencyUserHome } else { $UserHome }))
    $common = @{ SourceRoot = $SourceRoot; UserHome = $UserHome; CodexHome = $CodexHome; CodexCommand = $CodexCommand; DependencyUserHome = $DependencyUserHome }
    if ($Mode -eq 'Recover') { return Restore-HarnessActivation @common -PathScope $PathScope -Preview:$Preview }
    $null = Resolve-CodeToolsDependencyHome $UserHome $CodexHome $DependencyUserHome
    $pendingPath = Join-Path $CodexHome 'harness/activation-pending.json'
    if (Read-CodeToolsJson $pendingPath) { throw 'An incomplete combined activation needs install.ps1 -Mode Recover.' }
    $coreMode = if ($Mode -eq 'Update') { 'Install' } else { $Mode }
    $subscriptionPlan = Invoke-HarnessSubscriptionRouting @common -Mode $Mode -Preview
    $codePlan = Invoke-HarnessCodeTools @common -Mode $Mode -Preview
    $corePlan = Invoke-HarnessInstall @common -Mode $coreMode -PathScope $PathScope -IncludeCodeTools -Preview
    if ($Preview) {
        $corePlan | Add-Member -NotePropertyName codeTools -NotePropertyValue $codePlan -Force
        $corePlan | Add-Member -NotePropertyName subscriptions -NotePropertyValue $subscriptionPlan -Force
        return $corePlan
    }
    if ($Mode -eq 'Check') {
        $code = Invoke-HarnessCodeTools @common -Mode Check
        $corePlan | Add-Member -NotePropertyName codeTools -NotePropertyValue $code -Force
        $subscriptions = Invoke-HarnessSubscriptionRouting @common -Mode Check
        $corePlan | Add-Member -NotePropertyName subscriptions -NotePropertyValue $subscriptions -Force
        if ($code.status -ne 'protocol-ready') { $corePlan.status = 'Degraded' }
        if ($subscriptions.status -ne 'ready') { $corePlan.status = 'Degraded' }
        return $corePlan
    }
    $record = @{ schema_version = 1; owner = 'codex-harness-activation'; id = [guid]::NewGuid().ToString('N');
        mode = $Mode; phase = 'prepared'; source_root = [IO.Path]::GetFullPath($SourceRoot); user_home = [IO.Path]::GetFullPath($UserHome);
        codex_home = [IO.Path]::GetFullPath($CodexHome); dependency_user_home = $DependencyUserHome; recovery_errors = @() }
    Write-CodeToolsJson $pendingPath $record
    $commitWritten = $false
    try {
        if ($Mode -in @('Install','Update')) {
            Initialize-CodeToolsRuntime $DependencyUserHome $CodexHome $CodexCommand | Out-Null
            Initialize-CodeToolsRuntime $DependencyUserHome $CodexHome $CodexCommand -Tool graphify | Out-Null
        }
        $result = Invoke-HarnessInstall @common -Mode $coreMode -PathScope $PathScope -IncludeCodeTools -DeferCommit
        if ($Checkpoint) { & $Checkpoint 'core' }
        $codeResult = Invoke-HarnessCodeTools @common -Mode $Mode -DeferCommit -TransactionId $record.id -Checkpoint $Checkpoint
        $subscriptionResult = Invoke-HarnessSubscriptionRouting @common -Mode $Mode -DeferCommit -Checkpoint $Checkpoint
        if ($Checkpoint) { & $Checkpoint 'before-commit' }
        $record.phase = 'committed'
        Write-CodeToolsJson $pendingPath $record
        $commitWritten = $true
        if ($Checkpoint) { & $Checkpoint 'committed' }
        Complete-HarnessActivation $CodexHome
        $result | Add-Member -NotePropertyName codeTools -NotePropertyValue $codeResult -Force
        $result | Add-Member -NotePropertyName subscriptions -NotePropertyValue $subscriptionResult -Force
        $result
    } catch {
        $original = $_.Exception.Message
        if ($commitWritten) { throw "Activation committed; cleanup remains pending. Run Recover. Cause: $original" }
        try { Restore-HarnessActivation @common -PathScope $PathScope | Out-Null }
        catch { throw "Activation failed: $original. $($_.Exception.Message)" }
        throw "Activation failed and prior connections restored: $original"
    }
}
Export-ModuleMember -Function Invoke-HarnessActivation, Restore-HarnessActivation
