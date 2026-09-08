#requires -Version 7.4
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Journal destinations are fixed host-relative files; recovery needs no Python.
function Assert-CodeToolsPlain([string]$Path) {
    $current = [IO.Path]::GetFullPath($Path)
    while ($current) {
        $item = Get-Item -LiteralPath $current -Force -ErrorAction SilentlyContinue
        if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Preserving reparse path: $current" }
        $parent = Split-Path $current
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
function Read-CodeToolsJson([string]$Path) {
    Assert-CodeToolsPlain $Path
    if (Test-Path -LiteralPath $Path -PathType Leaf) {
        # Readers keep one complete snapshot while a journal writer atomically
        # replaces its path. Get-Content denies deletion on Windows and races
        # the service's readiness observer against the installer journal.
        try { $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, ([IO.FileShare]::Read -bor [IO.FileShare]::Delete)) }
        catch [IO.FileNotFoundException] { return } # A transaction may just have committed and removed its journal.
        try {
            $reader = [IO.StreamReader]::new($stream)
            try { $reader.ReadToEnd() | ConvertFrom-Json -AsHashtable }
            finally { $reader.Dispose() }
        } finally { $stream.Dispose() }
    }
}
function Get-CodeToolsBytes([string]$Path) {
    Assert-CodeToolsPlain $Path
    if (Test-Path -LiteralPath $Path -PathType Leaf) { ,([IO.File]::ReadAllBytes($Path)) } else { ,([byte[]]@()) }
}
function Get-CodeToolsHash([byte[]]$Bytes) {
    if ($null -eq $Bytes) { $Bytes = [byte[]]@() }
    [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($Bytes)).ToLowerInvariant()
}
function Write-CodeToolsBytes([string]$Path, [byte[]]$Bytes) {
    if ($null -eq $Bytes) { $Bytes = [byte[]]@() }
    Assert-CodeToolsPlain $Path
    [IO.Directory]::CreateDirectory((Split-Path $Path)) | Out-Null
    $temporary = $Path + '.' + [guid]::NewGuid().ToString('N') + '.tmp'
    try {
        $stream = [IO.File]::Open($temporary, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($Bytes); $stream.Flush($true) } finally { $stream.Dispose() }
        # Windows Move(overwrite) rejects even delete-sharing readers. Replace
        # preserves their old snapshot and the destination's access controls.
        if (Test-Path -LiteralPath $Path) { [IO.File]::Replace($temporary, $Path, [NullString]::Value) }
        else { [IO.File]::Move($temporary, $Path) }
    } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
}
function Write-CodeToolsJson([string]$Path, $Value) { Write-CodeToolsBytes $Path ([Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 60))) }
function Remove-CodeToolsFile([string]$Path) {
    Assert-CodeToolsPlain $Path
    if (Test-Path -LiteralPath $Path) { Remove-Item -LiteralPath $Path }
}

function Restore-CodeToolsRegistration([string]$CodexHome, [switch]$Preview) {
    $pendingPath = Join-Path $CodexHome 'harness/code-tools-registration-pending.json'
    $pending = Read-CodeToolsJson $pendingPath
    if (-not $pending) { return @{ status = 'no-pending-registration' } }
    $config = Join-Path $CodexHome 'config.toml'
    $state = Join-Path $CodexHome 'harness/code-tools-registration.json'
    $before = [Convert]::FromBase64String($pending.before)
    if ((Get-CodeToolsHash (Get-CodeToolsBytes $config)) -notin @((Get-CodeToolsHash $before), $pending.after_hash)) { throw 'Config changed after interrupted registration; preserving concurrent changes.' }
    $stateBefore = if ($pending.ContainsKey('previous_state_bytes') -and $null -ne $pending.previous_state_bytes) { [Convert]::FromBase64String($pending.previous_state_bytes) } elseif ($pending.previous_state) { [Text.UTF8Encoding]::new($false).GetBytes(($pending.previous_state | ConvertTo-Json -Depth 40)) } else { [byte[]]@() }
    if ($pending.ContainsKey('after_state_hash') -and (Get-CodeToolsHash (Get-CodeToolsBytes $state)) -notin @((Get-CodeToolsHash ([byte[]]$stateBefore)), $pending.after_state_hash)) { throw 'Registration state changed after interruption; preserving concurrent changes.' }
    if ($Preview) { return @{ status = 'preview-recovery' } }
    if ($pending.ContainsKey('config_existed') -and -not $pending.config_existed) { Remove-CodeToolsFile $config } else { Write-CodeToolsBytes $config $before }
    if (($pending.ContainsKey('previous_state_bytes') -and $null -ne $pending.previous_state_bytes) -or $pending.previous_state) { Write-CodeToolsBytes $state ([byte[]]$stateBefore) } else { Remove-CodeToolsFile $state }
    Remove-CodeToolsFile $pendingPath
    @{ status = 'registration-recovered' }
}

function Disconnect-CodeToolsRegistration([string]$CodexHome, [switch]$Preview, [switch]$DeferCommit,
    [string]$SourceRoot, [string]$UserHome, [string]$CodexCommand) {
    $statePath = Join-Path $CodexHome 'harness/code-tools-registration.json'
    $pendingPath = Join-Path $CodexHome 'harness/code-tools-registration-pending.json'
    if (Test-Path -LiteralPath $pendingPath) { throw 'Interrupted MCP registration: run install.ps1 -Mode Recover.' }
    $state = Read-CodeToolsJson $statePath
    if (-not $state) { return @{ status = 'unchanged-registration' } }
    if ($state.schema_version -ne 1 -or -not $state.block -or -not $state.registrations.Count) { throw 'Unknown or incomplete registration ownership state.' }
    $config = Join-Path $CodexHome 'config.toml'
    $before = Get-CodeToolsBytes $config
    # Literal byte removal preserves encoding and all unrelated settings.
    $encoded = [Convert]::ToHexString($before)
    $block = [Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes($state.block))
    $index = $encoded.IndexOf($block, [StringComparison]::Ordinal)
    $policyExact = $true
    $policyStart = 0
    $policyLength = 0
    if ($state.ContainsKey('connection_policy') -and $null -ne $state.connection_policy) {
        $policy = $state.connection_policy
        if ($policy.key -cne 'mcp_optional_startup_grace_ms' -or $policy.value -is [bool] -or $policy.value -ne 0 -or $policy.previous_present -isnot [bool]) { throw 'Unknown native MCP readiness ownership; preserving configuration.' }
        $statement = [Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes([string]$policy.statement))
        $prefix = ''
        $prefixMatches = $false
        if ($policy.ContainsKey('prefix_hash') -and $policy.ContainsKey('prefix_length')) {
            $prefixBytes = [long]$policy.prefix_length
            if ($prefixBytes -gt 0 -and $prefixBytes -le $before.Length) {
                $prefix = $encoded.Substring(0, [int]($prefixBytes * 2))
                $prefixMatches = (Get-CodeToolsHash ([Convert]::FromHexString($prefix))) -ceq $policy.prefix_hash
            }
        } elseif ($policy.ContainsKey('prefix')) {
            # Read the initial literal-prefix state without writing it again.
            $prefix = [Convert]::ToHexString([Text.Encoding]::UTF8.GetBytes([string]$policy.prefix))
            $prefixMatches = $encoded.StartsWith($prefix, [StringComparison]::Ordinal)
        }
        $policyExact = $prefix.Length -gt 0 -and $statement.Length -gt 0 -and $prefix.EndsWith($statement, [StringComparison]::Ordinal) -and $prefixMatches
        if (-not $policy.previous_present) {
            $policyStart = $prefix.Length - $statement.Length
            $policyLength = $statement.Length
            $policyExact = $policyExact -and $policyStart -ge 0 -and ($policyStart + $policyLength) -le $index
        }
    }
    if (-not $policyExact -or $index -lt 0 -or ($index % 2) -ne 0 -or $encoded.IndexOf($block, $index + $block.Length, [StringComparison]::Ordinal) -ge 0) {
        # Native TUI normalizes/reorders TOML and can interleave foreign sections
        # inside stale markers. A stdlib TOML parse proves every owned table and
        # isolates its statements; the adopted Serena environment is not required.
        $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
        if (-not $runtime.lifecycle_python) { throw 'Native-normalized MCP configuration needs an existing uv-managed Python for semantic ownership proof. No package is installed during Disconnect; current settings are preserved. Pending Recover remains available without Python.' }
        $native = if ($runtime.native) { $runtime.native } else { $runtime.powershell } # Disconnect never invokes the editor.
        $arguments = @((Join-Path $SourceRoot 'tools/code-tools/registration.py'), '--mode', 'Disconnect', '--codex-home', $CodexHome,
            '--source-root', $SourceRoot, '--native-codex', $native, '--powershell', $runtime.powershell)
        if ($Preview) { $arguments += '--preview' }
        if ($DeferCommit) { $arguments += '--defer-commit' }
        return Invoke-CodeToolsPythonJson $runtime.lifecycle_python $arguments
    }
    if ($Preview) { return @{ status = 'preview-disconnection' } }
    $remaining = $encoded.Remove($index, $block.Length)
    if ($policyLength) { $remaining = $remaining.Remove($policyStart, $policyLength) }
    $after = [Convert]::FromHexString($remaining)
    Write-CodeToolsJson $pendingPath @{ schema_version = 1; before = [Convert]::ToBase64String($before); after_hash = Get-CodeToolsHash $after;
        config_existed = $true; previous_state = $state; previous_state_bytes = [Convert]::ToBase64String((Get-CodeToolsBytes $statePath)); after_state_hash = Get-CodeToolsHash ([byte[]]@()) }
    if ((Get-CodeToolsHash (Get-CodeToolsBytes $config)) -ne (Get-CodeToolsHash $before)) { throw 'Config changed before disconnection; preserving concurrent changes.' }
    Write-CodeToolsBytes $config $after
    Remove-CodeToolsFile $statePath
    if (-not $DeferCommit) { Remove-CodeToolsFile $pendingPath }
    @{ status = 'disconnected' }
}

function Get-HarnessCodeToolsRuntime([string]$UserHome, [string]$CodexHome, [string]$CodexCommand) {
    $registryPath = Join-Path $CodexHome 'harness/code-tools.json'
    $record = Read-CodeToolsJson $registryPath
    $python = $null
    $graphifyPython = $null
    if ($record) {
        $serena = @($record.mcp | Where-Object id -EQ 'serena')
        if ($serena.Count -eq 1 -and $serena[0].paths.ContainsKey('python')) { $python = $serena[0].paths.python }
        $graphify = @($record.mcp | Where-Object id -EQ 'graphify')
        if ($graphify.Count -eq 1 -and $graphify[0].paths.ContainsKey('python')) { $graphifyPython = $graphify[0].paths.python }
    }
    $sameUser = [string]::Equals([IO.Path]::GetFullPath($UserHome).TrimEnd('\'), [Environment]::GetFolderPath('UserProfile').TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)
    $uvRoot = if ($env:UV_TOOL_DIR -and $sameUser) { $env:UV_TOOL_DIR } else { Join-Path $UserHome 'AppData/Roaming/uv/tools' }
    $uvBin = if ($env:UV_TOOL_BIN_DIR -and $sameUser) { $env:UV_TOOL_BIN_DIR } else { Join-Path $UserHome '.local/bin' }
    $pythonRoot = if ($env:UV_PYTHON_INSTALL_DIR -and $sameUser) { $env:UV_PYTHON_INSTALL_DIR } else { Join-Path $UserHome 'AppData/Roaming/uv/python' }
    if (-not $python -or -not (Test-Path -LiteralPath $python -PathType Leaf)) { $python = Join-Path $uvRoot 'serena-agent/Scripts/python.exe' }
    if (-not (Test-Path -LiteralPath $python -PathType Leaf)) { $python = $null }
    if (-not $graphifyPython -or -not (Test-Path -LiteralPath $graphifyPython -PathType Leaf)) { $graphifyPython = Join-Path $uvRoot 'graphifyy/Scripts/python.exe' }
    if (-not (Test-Path -LiteralPath $graphifyPython -PathType Leaf)) { $graphifyPython = $null }
    $uvCommand = Get-Command uv -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    $uv = if ($uvCommand) { $uvCommand.Source } else { $null }
    if (-not $uv -and (Test-Path -LiteralPath (Join-Path $UserHome '.local/bin/uv.exe') -PathType Leaf)) { $uv = Join-Path $UserHome '.local/bin/uv.exe' }
    $lifecyclePython = $python
    if (-not $lifecyclePython -and $uv) {
        # Only a known existing uv-managed interpreter, never PATH python or an
        # App Execution Alias. This command is offline and does not download.
        $priorPythonRoot = $env:UV_PYTHON_INSTALL_DIR
        try {
            $env:UV_PYTHON_INSTALL_DIR = $pythonRoot
            $found = & $uv python find --no-project --managed-python --offline --no-python-downloads --no-config '>=3.11' 2>$null
            if ($LASTEXITCODE -eq 0 -and $found -and (Test-Path -LiteralPath ([string]$found) -PathType Leaf)) { $lifecyclePython = [string]$found }
        } finally { $env:UV_PYTHON_INSTALL_DIR = $priorPythonRoot }
    }
    if (-not $CodexCommand) {
        $state = Read-CodeToolsJson (Join-Path $CodexHome 'harness/installation.json')
        if ($state) { $CodexCommand = $state.codexCommand }
    }
    if (-not $CodexCommand) { $foundCodex = Get-Command codex -ErrorAction SilentlyContinue; if ($foundCodex) { $CodexCommand = $foundCodex.Source } }
    $native = $CodexCommand
    if ($native -and [IO.Path]::GetExtension($native) -ne '.exe') {
        $vendor = Join-Path (Split-Path $native) 'node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor'
        $candidates = @(if (Test-Path -LiteralPath $vendor) { Get-ChildItem -LiteralPath $vendor -Recurse -File -Filter 'codex.exe' })
        $native = if ($candidates.Count -eq 1) { $candidates[0].FullName } else { $null }
    }
    @{ python = $python; graphify_python = $graphifyPython; lifecycle_python = $lifecyclePython; uv = $uv; uv_root = $uvRoot; uv_bin = $uvBin; python_root = $pythonRoot; native = $native;
        powershell = (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source; registry = $registryPath }
}

function Invoke-CodeToolsPythonJson([string]$Python, [string[]]$Arguments) {
    if (-not $Python) { throw 'No existing uv-managed Python >=3.11 for stdlib dependency lifecycle. Recover and Disconnect of links/registrations remain available.' }
    $raw = & $Python -B @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Code-tools operation failed (exit $LASTEXITCODE): $([IO.Path]::GetFileName($Arguments[0])). See original diagnostic; pending recovery records are retained." }
    ($raw -join [Environment]::NewLine) | ConvertFrom-Json -AsHashtable
}

function Restore-CodeToolsRegistries([string]$CodexHome, [switch]$Preview) {
    $pendingPath = Join-Path $CodexHome 'harness/code-tools-files-pending.json'
    $pending = Read-CodeToolsJson $pendingPath
    if (-not $pending) { return }
    if ($pending.schema_version -ne 1) { throw 'Unknown registry recovery schema.' }
    foreach ($file in $pending.files) {
        if ($file.name -notin @('code-tools.json','lsp-servers.json')) { throw 'Unowned registry recovery destination.' }
        $path = Join-Path $CodexHome ('harness/' + $file.name)
        $before = if ($null -ne $file.before) { [Convert]::FromBase64String($file.before) } else { [byte[]]@() }
        if ((Get-CodeToolsHash (Get-CodeToolsBytes $path)) -notin @((Get-CodeToolsHash ([byte[]]$before)), $file.after_hash)) { throw "Registry changed after interruption; preserving: $($file.name)" }
    }
    if ($Preview) { return }
    foreach ($file in $pending.files) {
        $path = Join-Path $CodexHome ('harness/' + $file.name)
        if ($null -ne $file.before) { Write-CodeToolsBytes $path ([Convert]::FromBase64String($file.before)) } else { Remove-CodeToolsFile $path }
    }
    Remove-CodeToolsFile $pendingPath
}

function Write-CodeToolsRegistries([string]$CodexHome, $Inventory, $LspRegistry, [scriptblock]$Checkpoint) {
    $records = @(); $outputs = @{ 'code-tools.json' = $Inventory; 'lsp-servers.json' = $LspRegistry }
    foreach ($name in @('code-tools.json','lsp-servers.json')) {
        $path = Join-Path $CodexHome ('harness/' + $name)
        $after = [Text.UTF8Encoding]::new($false).GetBytes(($outputs[$name] | ConvertTo-Json -Depth 60))
        $records += @{ name = $name; before = if (Test-Path -LiteralPath $path) { [Convert]::ToBase64String((Get-CodeToolsBytes $path)) } else { $null }; after_hash = Get-CodeToolsHash $after; after = [Convert]::ToBase64String($after) }
    }
    $pendingPath = Join-Path $CodexHome 'harness/code-tools-files-pending.json'
    if (Test-Path -LiteralPath $pendingPath) { throw 'Interrupted registry activation: run Recover.' }
    Write-CodeToolsJson $pendingPath @{ schema_version = 1; files = $records }
    foreach ($file in $records) {
        $path = Join-Path $CodexHome ('harness/' + $file.name)
        $before = if ($null -ne $file.before) { [Convert]::FromBase64String($file.before) } else { [byte[]]@() }
        if ((Get-CodeToolsHash (Get-CodeToolsBytes $path)) -ne (Get-CodeToolsHash ([byte[]]$before))) { throw "Registry changed before activation; preserving: $($file.name)" }
        Write-CodeToolsBytes $path ([Convert]::FromBase64String($file.after))
        if ($Checkpoint) { & $Checkpoint ('registry:' + $file.name) }
    }
}

function Resolve-CodeToolsDependencyHome([string]$UserHome, [string]$CodexHome, [string]$DependencyUserHome) {
    $selected = [IO.Path]::GetFullPath($(if ($DependencyUserHome) { $DependencyUserHome } else { $UserHome })).TrimEnd('\')
    $state = Read-CodeToolsJson (Join-Path $CodexHome 'harness/installation.json')
    if ($state -and $state.ContainsKey('dependencyUserHome') -and -not [string]::Equals($selected, [IO.Path]::GetFullPath($state.dependencyUserHome).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { throw 'Dependency owner differs from the recorded installation; preserve its connections.' }
    $registry = Read-CodeToolsJson (Join-Path $CodexHome 'harness/code-tools.json')
    if ($registry -and $registry.ContainsKey('user_home') -and -not [string]::Equals($selected, [IO.Path]::GetFullPath($registry.user_home).TrimEnd('\'), [StringComparison]::OrdinalIgnoreCase)) { throw 'Dependency owner differs from the adopted registry; preserve its selected installations.' }
    return $selected
}

function Invoke-HarnessCodeTools {
    [CmdletBinding()]
    param([string]$SourceRoot, [string]$UserHome, [string]$CodexHome, [string]$CodexCommand,
        [ValidateSet('Install','Update','Check','Disconnect','Recover')][string]$Mode, [switch]$Preview,
        [switch]$DeferCommit, [string]$TransactionId, [scriptblock]$Checkpoint, [string]$DependencyUserHome,
        [switch]$SkipDependencyChanges)
    $DependencyUserHome = Resolve-CodeToolsDependencyHome $UserHome $CodexHome $DependencyUserHome
    if ($Mode -eq 'Recover') {
        Invoke-CodeToolsResources $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Mode recover -Preview:$Preview | Out-Null
        return Restore-CodeToolsRegistration $CodexHome -Preview:$Preview
    }
    if ($Mode -eq 'Disconnect') {
        Stop-CodeToolsServices $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Preview:$Preview
        Invoke-CodeToolsResources $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Mode restore -Preview:$Preview -DeferCommit:$DeferCommit -TransactionId $TransactionId | Out-Null
        return Disconnect-CodeToolsRegistration $CodexHome -Preview:$Preview -DeferCommit:$DeferCommit -SourceRoot $SourceRoot -UserHome $DependencyUserHome -CodexCommand $CodexCommand
    }
    $runtime = Get-HarnessCodeToolsRuntime $DependencyUserHome $CodexHome $CodexCommand
    if ($Mode -eq 'Check' -and -not $runtime.python) { return @{ status = 'degraded'; reason = 'Adopted Serena environment is missing; MCP health could not run. Recover and Disconnect remain available without Python.'; callable = $false } }
    if (-not $runtime.lifecycle_python) {
        if ($Preview) { return @{ status = 'preview-bootstrap'; reason = 'Install will explicitly provision missing uv, managed Python and Serena; preview installs nothing.' } }
        throw 'Dependency lifecycle needs uv and Python >=3.11; no compatible existing interpreter was found.'
    }
    if (-not $runtime.native) { throw 'Cannot resolve native Codex executable; pass -CodexCommand with the real codex.exe.' }
    $dependencies = $null
    $resourceHealth = $null
    if ($Mode -in @('Install','Update') -and -not $SkipDependencyChanges) {
        Stop-CodeToolsServices $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Preview:$Preview
        $operation = if ($Preview) { 'plan' } elseif ($Mode -eq 'Update') { 'update' } else { 'apply' }
        $dependencyArgs = @((Join-Path $SourceRoot 'tools/code-tools/dependencies.py'), $operation, '--user-home', $DependencyUserHome, '--state-dir', (Join-Path $CodexHome 'harness/dependencies'), '--codex-home', $CodexHome)
        if ($TransactionId) { $dependencyArgs += @('--transaction-id', $TransactionId) }
        $dependencies = Invoke-CodeToolsPythonJson $runtime.lifecycle_python $dependencyArgs
        if (-not $Preview) {
            Write-CodeToolsJson (Join-Path $CodexHome ('harness/dependencies/result-' + $TransactionId + '.json')) $dependencies
            $failed = @($dependencies.results | Where-Object { $_.state -in @('failed','pending') })
            if ($failed.Count) { throw "Dependency lifecycle has unresolved requirements: $(($failed.id | Sort-Object -Unique) -join ', '). Inspect the machine-local dependency result." }
        }
    }
    $discoveryArgs = @((Join-Path $SourceRoot 'tools/code-tools/discovery.py'), '--user-home', $DependencyUserHome, '--verify-records')
    if ($Mode -in @('Install','Update')) { $discoveryArgs += '--probe-versions' }
    $inventory = Invoke-CodeToolsPythonJson $runtime.lifecycle_python $discoveryArgs
    $registrationArgs = @((Join-Path $SourceRoot 'tools/code-tools/registration.py'), '--codex-home', $CodexHome, '--source-root', $SourceRoot,
        '--native-codex', $runtime.native, '--powershell', $runtime.powershell, '--mode', $Mode)
    $serenaEntries = @($inventory.mcp | Where-Object id -EQ 'serena')
    $mcpPython = if ($serenaEntries.Count) { $serenaEntries[0].paths.python } else { $null }
    if ($mcpPython) { $registrationArgs += @('--python', $mcpPython) }
    if ($Preview) { $registrationArgs += '--preview' }
    if ($DeferCommit) { $registrationArgs += '--defer-commit' }
    $registration = Invoke-CodeToolsPythonJson $runtime.lifecycle_python $registrationArgs
    if ($Checkpoint -and -not $Preview) { & $Checkpoint 'registration' }
    if (-not $Preview -and $Mode -in @('Install','Update')) {
        # Explicit Serena dependencies remain discoverable to its runtime guard.
        # They do not select the retired, separately owned harness LSP backend.
        $lsp = @{ schema_version = 1; servers = @{} }
        Write-CodeToolsRegistries $CodexHome $inventory $lsp $Checkpoint
        $resourceHealth = Invoke-CodeToolsResources $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Mode apply -DeferCommit:$DeferCommit -TransactionId $TransactionId
        if ($Checkpoint) { & $Checkpoint 'resources' }
        if (-not $DeferCommit) { Remove-CodeToolsFile (Join-Path $CodexHome 'harness/code-tools-files-pending.json') }
    }
    $health = $null
    if (-not $Preview -and $Mode -eq 'Check' -and (Test-Path -LiteralPath $runtime.registry)) {
        try { $resourceHealth = Invoke-CodeToolsResources $SourceRoot $DependencyUserHome $CodexHome $CodexCommand -Mode check }
        catch { $resourceHealth = @{ status = 'degraded'; reason = $_.Exception.Message } }
        $health = Invoke-CodeToolsPythonJson $runtime.python @((Join-Path $SourceRoot 'tools/code-tools/check.py'), '--registry', $runtime.registry, '--codex-home', $CodexHome)
    }
    @{ registration = $registration; inventory = $inventory; dependencies = $dependencies; health = $health; resources = $resourceHealth;
        status = if ($Preview) { 'Preview code tools' } elseif ($Mode -eq 'Check' -and ($registration.status -ne 'connected' -or ($resourceHealth -and $resourceHealth.status -eq 'degraded'))) { 'degraded' } elseif ($health) { $health.status } else { $registration.status } }
}

function Stop-CodeToolsServices([string]$SourceRoot, [string]$UserHome, [string]$CodexHome, [string]$CodexCommand, [switch]$Preview) {
    if ($Preview) { return }
    $runtime = $null
    $previousHome = $env:CODEX_HOME
    try {
        $env:CODEX_HOME = $CodexHome
        foreach ($entry in @(@('lsp-broker','tools/lsp/broker.py'), @('serena-broker','tools/code-tools/serena_broker.py'))) {
            $directory = Join-Path $CodexHome ('harness/runtime/' + $entry[0])
            # No import or interpreter requirement when this installation has
            # never run a shared service. Existing directories require a probe:
            # an exclusive owner can still be starting without a ready receipt.
            if (-not (Test-Path -LiteralPath $directory)) { continue }
            if (-not $runtime) { $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand }
            if (-not $runtime.python) { throw 'An existing shared service needs its adopted Python to retire; preserving dependencies and connections.' }
            $result = Invoke-CodeToolsPythonJson $runtime.python @((Join-Path $SourceRoot $entry[1]), '--retire')
            if ($result.status -notin @('retired','not-running','absent')) { throw "Shared service retirement remains pending: $($entry[0])" }
        }
    } finally { $env:CODEX_HOME = $previousHome }
}

function Invoke-CodeToolsResources {
    param([string]$SourceRoot, [string]$UserHome, [string]$CodexHome, [string]$CodexCommand,
        [ValidateSet('apply','check','restore','recover','commit')][string]$Mode,
        [switch]$Preview, [switch]$DeferCommit, [string]$TransactionId)
    $pending = Join-Path $CodexHome 'harness/tool-resources-pending.json'
    if ($Mode -in @('recover','commit') -and -not (Test-Path -LiteralPath $pending)) { return @{ status = 'not-pending' } }
    if ($Preview) { return @{ status = 'preview'; operation = $Mode } }
    $alternateOwner = -not [string]::Equals([IO.Path]::GetFullPath($UserHome), [Environment]::GetFolderPath('UserProfile'), [StringComparison]::OrdinalIgnoreCase)
    $stateDirectory = if ($alternateOwner) { Join-Path $UserHome 'AppData/Local/codex-tool-resources' } elseif ($env:HARNESS_TOOL_RESOURCES_DIR) { $env:HARNESS_TOOL_RESOURCES_DIR } else { Join-Path $env:LOCALAPPDATA 'codex-tool-resources' }
    if ($Mode -eq 'restore' -and -not (Test-Path -LiteralPath (Join-Path $stateDirectory 'cbm-configuration.json')) -and -not (Test-Path -LiteralPath $pending)) { return @{ status = 'not-owned' } }
    $runtime = Get-HarnessCodeToolsRuntime $UserHome $CodexHome $CodexCommand
    $arguments = @((Join-Path $SourceRoot 'tools/code-tools/resources.py'), $Mode, '--registry', $runtime.registry, '--pending', $pending, '--owner', $CodexHome)
    # Isolated installation fixtures and explicit alternate dependency owners
    # must never fall through to the invoking account's live native settings.
    if ($alternateOwner) {
        $arguments += @('--state-dir', (Join-Path $UserHome 'AppData/Local/codex-tool-resources'), '--cache-dir', (Join-Path $UserHome '.cache/codebase-memory-mcp'))
    }
    if ($TransactionId) { $arguments += @('--transaction-id', $TransactionId) }
    if ($DeferCommit) { $arguments += '--defer-commit' }
    if ($Mode -in @('apply','check')) {
        $inventory = Read-CodeToolsJson $runtime.registry
        if (-not $inventory -or -not @($inventory.mcp | Where-Object id -EQ 'codebase-memory').Count) {
            return @{ status = 'unavailable'; reason = 'No discovered Codebase Memory executable; resource configuration was not mutated.' }
        }
    }
    if (-not $runtime.lifecycle_python) { throw 'Owned resource settings need an existing Python runtime for compare-and-restore recovery; the pending journal and user configuration are preserved.' }
    Invoke-CodeToolsPythonJson $runtime.lifecycle_python $arguments
}

Export-ModuleMember -Function Invoke-HarnessCodeTools, Get-HarnessCodeToolsRuntime, Invoke-CodeToolsPythonJson, Restore-CodeToolsRegistries,
    Restore-CodeToolsRegistration, Read-CodeToolsJson, Write-CodeToolsJson, Remove-CodeToolsFile, Write-CodeToolsRegistries,
    Assert-CodeToolsPlain, Get-CodeToolsBytes, Write-CodeToolsBytes, Get-CodeToolsHash, Resolve-CodeToolsDependencyHome,
    Stop-CodeToolsServices, Invoke-CodeToolsResources
