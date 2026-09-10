#requires -Version 7.4
# Standalone Windows lifecycle checks. Fixtures copy source only to simulate a
# separate checkout. Shared resources stay linked; the resilient command
# bootstrap is an explicit installer-owned copy independent of that checkout.
[CmdletBinding()]
param([string] $CodexCommand)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repository = Split-Path $PSScriptRoot -Parent
$modulePath = Join-Path $repository 'tools/kit.psm1'
if (-not $CodexCommand) { $CodexCommand = (Get-Command codex -ErrorAction Stop).Source }
$pwsh = (Get-Command pwsh -CommandType Application | Select-Object -First 1).Source
$suiteRoot = Join-Path ([IO.Path]::GetTempPath()) ('codex-installer проба ' + [guid]::NewGuid().ToString('N'))
$initialPath = $env:Path
$failures = [Collections.Generic.List[string]]::new()
$assertions = 0
$cases = 0
$verificationResources = @{
    'project-verification' = @('SKILL.md', 'references/command-records.md')
    'reproduce-regression' = @('SKILL.md', 'references/reduction.md', 'references/process-fixtures.md', 'scripts/process_case.py', 'scripts/observe.ps1')
}

function Assert-True([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
    $script:assertions++
}
function Assert-Throw([scriptblock] $Action, [string] $Pattern) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_.Exception.Message }
    Assert-True ($null -ne $caught -and $caught -match $Pattern) "Expected failure /$Pattern/; observed: $caught"
}
function Write-FixtureFile([string] $Path, [string] $Body) {
    New-Item -ItemType Directory -Path (Split-Path $Path) -Force | Out-Null
    Set-Content -LiteralPath $Path -Value $Body -Encoding utf8
}
function New-Fixture {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Creates only the disposable suite fixture; lifecycle assertions require these writes without a new approval path.')]
    param([string] $Name)
    $root = Join-Path $suiteRoot $Name
    $source = Join-Path $root 'checkout источник'
    $manifest = Import-PowerShellDataFile (Join-Path $repository 'global/kit.psd1')
    foreach ($relative in @($manifest.RequiredFiles) + @('global/kit.psd1', $manifest.Profile, $manifest.Instructions)) {
        $destination = Join-Path $source $relative
        New-Item -ItemType Directory -Path (Split-Path $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $repository $relative) -Destination $destination
    }
    foreach ($relative in @($manifest.Skills, $manifest.Agents)) {
        $destination = Join-Path $source $relative
        New-Item -ItemType Directory -Path $destination -Force | Out-Null
        foreach ($item in Get-ChildItem -LiteralPath (Join-Path $repository $relative) -Force) {
            Copy-Item -LiteralPath $item.FullName -Destination $destination -Recurse
        }
    }
    @{ SourceRoot = $source; CodexHome = Join-Path $root 'codex'; UserHome = Join-Path $root 'user'; CodexCommand = $CodexCommand; PathScope = 'Process' }
}
function Reset-TestModule {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private test setup replaces module mocks in this process only; it must run deterministically.')]
    param([switch] $RealRuntime)
    $script:testModule = Import-Module $modulePath -Force -PassThru
    if (-not $RealRuntime) {
        & $script:testModule {
            function script:Assert-HarnessPrerequisites {
                [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseSingularNouns', '', Justification = 'Mock name must match the production command being overridden.')]
                param()
                @{ codex = 'lifecycle stub'; openspec = 'lifecycle stub'; powershell = $PSVersionTable.PSVersion.ToString() }
            }
            function script:Test-HarnessRuntime { @{ evidence = 'Lifecycle stub only; actual startup covered separately.' } }
        }
    }
}
function Read-State($Fixture) {
    Get-Content -LiteralPath (Join-Path $Fixture.CodexHome 'harness/installation.json') -Raw | ConvertFrom-Json -AsHashtable
}
function Assert-DiagnosticLink($Fixture) {
    $link=@((Read-State $Fixture).links | Where-Object kind -eq 'diagnostic-launcher')
    Assert-True ($link.Count -eq 1 -and $link[0].owned) 'Diagnostic launcher must be registered with ownership.'
    $item=Get-Item -LiteralPath $link[0].destination -Force
    Assert-True ($item.LinkType -eq 'SymbolicLink' -and $item.Target -eq (Join-Path $Fixture.SourceRoot 'tools/codex-harness-check.ps1')) 'Diagnostic launcher must link directly to the selected source.'
}
function Assert-VerificationResource($Fixture, [string] $Marker) {
    $state = Read-State $Fixture
    foreach ($name in $verificationResources.Keys) {
        $source = Join-Path $Fixture.SourceRoot ".agents/skills/$name"
        $destination = Join-Path $Fixture.UserHome ".agents/skills/$name"
        $links = @($state.links | Where-Object destination -eq $destination)
        Assert-True ($links.Count -eq 1 -and $links[0].owned) "$name must have one owned registration."
        $item = Get-Item -LiteralPath $destination -Force
        Assert-True ($item.LinkType -eq 'SymbolicLink' -and $item.Target -eq $source) "$name must link directly to its source directory."
        foreach ($relative in $verificationResources[$name]) {
            $sourceFile = Join-Path $source $relative
            $installedFile = Join-Path $destination $relative
            $body = Get-Content -LiteralPath $installedFile -Raw
            Assert-True (-not [string]::IsNullOrWhiteSpace($body)) "$name/$relative must be readable through the installed link."
            Assert-True ((Get-FileHash -LiteralPath $sourceFile).Hash -eq (Get-FileHash -LiteralPath $installedFile).Hash) "$name/$relative must expose the authoritative bytes."
            if ($Marker) { Assert-True ($body.Contains($Marker)) "$name/$relative must expose the expected source revision."
            }
        }
    }
    foreach ($relative in 'tools/opencodex-process.ps1', 'tools/opencodex-process.cs') {
        Assert-True (Test-Path -LiteralPath (Join-Path $Fixture.SourceRoot $relative) -PathType Leaf) "Linked process helper dependency must be portable: $relative"
    }
}
function Write-Pending($Fixture, $Pending) {
    Write-FixtureFile (Join-Path $Fixture.CodexHome 'harness/pending.json') ($Pending | ConvertTo-Json -Depth 30)
}
function Get-PendingFromInstalled($Fixture, [string] $BeforePath) {
    $state = Read-State $Fixture
    @{ previousState = $null; plannedState = $state; pathScope = 'Process'; pathBefore = $BeforePath; pathAfter = $env:Path;
        operations = @($state.links | Where-Object owned | ForEach-Object { @{ destination = $_.destination; oldSource = $null; newSource = $_.source } }) }
}
function Test-Case([string] $Name, [scriptblock] $Action) {
    $before = $env:Path
    try {
        Reset-TestModule
        & $Action
        $script:cases++
        Write-Output "PASS: $Name"
    } catch {
        $failures.Add("${Name}: $($_.Exception.Message)")
        Write-Output "FAIL: ${Name}: $($_.Exception.Message)"
    } finally { $env:Path = $before }
}
function Remove-FixtureTree {
    [Diagnostics.CodeAnalysis.SuppressMessageAttribute('PSUseShouldProcessForStateChangingFunctions', '', Justification = 'Private cleanup validates every target within the owned suite and handles reparse points before traversal.')]
    param([string] $Path)
    # Check every absolute target; handle reparse points before directory walking.
    $full = [IO.Path]::GetFullPath($Path)
    $prefix = [IO.Path]::GetFullPath($suiteRoot).TrimEnd('\')
    if ($full -ne $prefix -and -not $full.StartsWith($prefix + '\', [StringComparison]::OrdinalIgnoreCase)) { throw "Fixture cleanup escaped suite: $full" }
    $item = Get-Item -LiteralPath $full -Force -ErrorAction SilentlyContinue
    if (-not $item) { return }
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { Remove-Item -LiteralPath $full -Force; return }
    if ($item.PSIsContainer) { foreach ($child in Get-ChildItem -LiteralPath $full -Force) { Remove-FixtureTree $child.FullName } }
    Remove-Item -LiteralPath $full -Force
}

try {
    Test-Case 'core updates preserve connected hooks through repeat relocation repair and conflict' {
        $fixture = New-Fixture 'core-hooks'
        Invoke-HarnessInstall @fixture | Out-Null
        Assert-True (@((Read-State $fixture).links | Where-Object kind -in 'hooks','hook-launcher').Count -eq 0) 'Fresh core install must not activate hooks.'
        Invoke-HarnessInstall @fixture -IncludeCodeTools | Out-Null
        $hookLinks = @((Read-State $fixture).links | Where-Object kind -in 'hooks','hook-launcher')
        Assert-True ($hookLinks.Count -eq 2) 'Fixture must begin with both connected hooks.'
        $preview = Invoke-HarnessInstall @fixture -Preview
        Assert-True (@($preview.operations | Where-Object destination -in $hookLinks.destination).Count -eq 0) 'Core update preview must not remove connected hooks.'
        foreach ($attempt in 1..2) {
            Invoke-HarnessInstall @fixture | Out-Null
            Assert-True (@((Read-State $fixture).links | Where-Object kind -in 'hooks','hook-launcher').Count -eq 2) 'Core update must keep both hook records.'
            foreach ($link in $hookLinks) { Assert-True (Test-Path -LiteralPath $link.destination -PathType Leaf) 'Core update must keep each hook usable.' }
        }
        $moved = New-Fixture 'core-hooks-moved'
        $fixture.SourceRoot = $moved.SourceRoot
        Invoke-HarnessInstall @fixture | Out-Null
        $hookLinks = @((Read-State $fixture).links | Where-Object kind -in 'hooks','hook-launcher')
        foreach ($link in $hookLinks) {
            Assert-True ($link.source.StartsWith($fixture.SourceRoot + '\')) 'Hook source must follow checkout relocation.'
            Assert-True ((Get-Item -LiteralPath $link.destination).LinkTarget -eq $link.source) 'Hook must link directly to relocated source.'
        }
        $missing = $hookLinks[0]
        Remove-Item -LiteralPath $missing.destination -Force
        Invoke-HarnessInstall @fixture | Out-Null
        Assert-True ((Get-Item -LiteralPath $missing.destination).LinkTarget -eq $missing.source) 'Core update must repair a missing recorded hook.'
        Remove-Item -LiteralPath $missing.destination -Force
        Write-FixtureFile $missing.destination 'foreign hook bytes'
        $statePath = Join-Path $fixture.CodexHome 'harness/installation.json'
        $stateHash = (Get-FileHash -LiteralPath $statePath).Hash
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Target conflict; preserving'
        Assert-True ((Get-Content -LiteralPath $missing.destination -Raw).Trim() -eq 'foreign hook bytes') 'Foreign replacement must survive failed update.'
        Assert-True ((Get-FileHash -LiteralPath $statePath).Hash -eq $stateHash) 'Conflict must preserve installation metadata.'
        Remove-Item -LiteralPath $missing.destination -Force
        New-Item -ItemType SymbolicLink -Path $missing.destination -Target $missing.source | Out-Null
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        foreach ($link in $hookLinks) { Assert-True (-not (Test-Path -LiteralPath $link.destination)) 'Explicit disconnect must still remove owned hooks.' }
    }

    Test-Case 'actual install.ps1 startup from a separate checkout with spaces and Cyrillic' {
        $fixture = New-Fixture 'actual'
        Reset-TestModule -RealRuntime
        $entry = Join-Path $fixture.SourceRoot 'install.ps1'
        $preview = & $entry -CoreOnly -CodexHome $fixture.CodexHome -UserHome $fixture.UserHome -CodexCommand $CodexCommand -PathScope Process -WhatIf
        Assert-True ($preview.status -eq 'Preview Install') 'Preview must return an explicit non-activation result.'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Preview must not create the target home.'
        $result = & $entry -CoreOnly -CodexHome $fixture.CodexHome -UserHome $fixture.UserHome -CodexCommand $CodexCommand -PathScope Process
        Assert-DiagnosticLink $fixture
        Assert-True ($result.runtime.fullGlobalInstructions -and $result.runtime.sandbox -eq 'danger-full-access') 'Real fresh Codex process must read instructions and Full Access.'
        $check = & $entry -CoreOnly -CodexHome $fixture.CodexHome -UserHome $fixture.UserHome -Mode Check
        Assert-True ($check.status -eq 'Connected') 'Actual Check must exercise the connected CLI.'
        Add-Content -LiteralPath (Join-Path $fixture.SourceRoot 'global/principles-of-work.md') -Value "`nDisposable installer test marker: live source update."
        $check = & $entry -CoreOnly -CodexHome $fixture.CodexHome -UserHome $fixture.UserHome -Mode Check
        Assert-True ($check.runtime.fullGlobalInstructions) 'New Codex process must read edited instructions without reinstall.'
        & $entry -CoreOnly -CodexHome $fixture.CodexHome -UserHome $fixture.UserHome -Mode Disconnect | Out-Null
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/bin/codex-harness-check.ps1'))) 'Disconnect must remove its owned diagnostic link.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json'))) 'Disconnect must remove installation state.'
        Assert-True (Test-Path -LiteralPath $CodexCommand) 'Original CLI must remain present.'
    }

    Test-Case 'repeat install, preview, direct sources, unrelated state, and disconnect' {
        $fixture = New-Fixture 'lifecycle'
        Write-FixtureFile (Join-Path $fixture.CodexHome 'config.toml') 'test_local_marker = "keep"'
        Write-FixtureFile (Join-Path $fixture.CodexHome 'auth.fixture') 'not real credentials'
        Write-FixtureFile (Join-Path $fixture.UserHome '.agents/skills/foreign/SKILL.md') "---`nname: unrelated-capability`ndescription: retained`n---"
        $before = $env:Path
        Invoke-HarnessInstall @fixture | Out-Null
        $state = Read-State $fixture
        Assert-DiagnosticLink $fixture
        foreach ($link in $state.links) {
            $item = Get-Item -LiteralPath $link.destination -Force
            Assert-True ($item.LinkType -eq 'SymbolicLink' -and $item.Target -eq $link.source) 'Every managed artifact must be a direct source link.'
        }
        $connectedPath = $env:Path
        $preview = Invoke-HarnessInstall @fixture -Preview
        Assert-True ($preview.operations.Count -eq 0 -and -not $preview.pathChange) 'Repeat preview must be empty.'
        Invoke-HarnessInstall @fixture | Out-Null
        Assert-True ($env:Path -ceq $connectedPath) 'Repeat install must not duplicate PATH.'
        Assert-DiagnosticLink $fixture
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        Assert-True ($env:Path -ceq $before) 'Disconnect must restore only its PATH registration.'
        Assert-True ((Get-Content -LiteralPath (Join-Path $fixture.CodexHome 'config.toml') -Raw).Contains('keep')) 'Local config must survive.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'auth.fixture')) 'Local auth fixture must survive.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.UserHome '.agents/skills/foreign/SKILL.md')) 'Foreign skill must survive.'
        foreach ($link in $state.links) { Assert-True (Test-Path -LiteralPath $link.source) 'Disconnect must preserve source.' }
    }

    Test-Case 'verification skill resources update directly and survive disconnect/reconnect' {
        $fixture = New-Fixture 'verification-resources'
        $foreign = Join-Path $fixture.UserHome '.agents/skills/foreign-resource/references/keep.md'
        Write-FixtureFile $foreign 'unrelated resource'
        $config = Join-Path $fixture.CodexHome 'config.toml'
        Write-FixtureFile $config '# unrelated verification fixture config'
        Invoke-HarnessInstall @fixture | Out-Null
        $installedConfig = Get-Content -LiteralPath $config -Raw
        Assert-True ($installedConfig.Contains('# unrelated verification fixture config')) 'Hook selection must preserve the unrelated configuration text.'
        Assert-True ($installedConfig -match '(?m)^hooks = false$') 'Fresh install must persist the disabled base hook feature.'
        $configHash = (Get-FileHash -LiteralPath $config).Hash
        Assert-VerificationResource $fixture
        $marker = '# disposable verification resource revision two'
        foreach ($name in $verificationResources.Keys) {
            foreach ($relative in $verificationResources[$name]) {
                Add-Content -LiteralPath (Join-Path $fixture.SourceRoot ".agents/skills/$name/$relative") -Value $marker
            }
        }
        # Reading before reinstall distinguishes live source links from deployment copies.
        Assert-VerificationResource $fixture $marker
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        foreach ($name in $verificationResources.Keys) {
            $destination = Join-Path $fixture.UserHome ".agents/skills/$name"
            Assert-True (-not [bool](Get-Item -LiteralPath $destination -Force -ErrorAction SilentlyContinue)) "$name owned link must be removed."
            foreach ($relative in $verificationResources[$name]) {
                Assert-True ((Get-Content -LiteralPath (Join-Path $fixture.SourceRoot ".agents/skills/$name/$relative") -Raw).Contains($marker)) "$name/$relative source must survive disconnect."
            }
        }
        Assert-True ((Get-Content -LiteralPath $foreign -Raw).Trim() -eq 'unrelated resource') 'Disconnect must preserve unrelated skill resources.'
        Assert-True ((Get-FileHash -LiteralPath $config).Hash -eq $configHash) 'Disconnect must preserve unrelated configuration bytes.'
        Invoke-HarnessInstall @fixture | Out-Null
        Assert-VerificationResource $fixture $marker
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        Assert-True (Test-Path -LiteralPath $foreign) 'Reconnect/disconnect must preserve the foreign resource.'
    }

    Test-Case 'both verification skills preserve foreign directories and linked resources on collision' {
        foreach ($name in $verificationResources.Keys) {
            foreach ($kind in 'directory', 'link') {
                $fixture = New-Fixture "verification-conflict-$name-$kind"
                $destination = Join-Path $fixture.UserHome ".agents/skills/$name"
                $foreign = if ($kind -eq 'link') { Join-Path (Split-Path $fixture.SourceRoot) 'foreign skill источник' } else { $destination }
                Write-FixtureFile (Join-Path $foreign 'SKILL.md') "---`nname: $name`ndescription: foreign fixture`n---"
                $resource = Join-Path $foreign 'references/keep.md'
                Write-FixtureFile $resource 'foreign reference bytes'
                if ($kind -eq 'link') {
                    New-Item -ItemType Directory -Path (Split-Path $destination) -Force | Out-Null
                    New-Item -ItemType SymbolicLink -Path $destination -Target $foreign | Out-Null
                }
                $before = $env:Path
                Assert-Throw { Invoke-HarnessInstall @fixture -Preview } 'Target conflict|name collision'
                Assert-Throw { Invoke-HarnessInstall @fixture } 'Target conflict|name collision'
                Assert-True ($env:Path -ceq $before) 'Skill collision must not change PATH.'
                Assert-True ((Get-Content -LiteralPath (Join-Path $destination 'references/keep.md') -Raw).Trim() -eq 'foreign reference bytes') "$name collision must preserve readable foreign resources."
                $item = Get-Item -LiteralPath $destination -Force
                if ($kind -eq 'link') {
                    Assert-True ($item.LinkType -eq 'SymbolicLink' -and $item.Target -eq $foreign) 'Foreign skill link target must remain unchanged.'
                } else {
                    Assert-True (-not ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) 'Foreign skill directory must not be replaced with a link.'
                }
                Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness'))) 'Skill collision must fail before activation.'
            }
        }
    }

    Test-Case 'foreign target file, link, and higher-priority instructions are preserved' {
        foreach ($kind in 'file', 'link', 'override') {
            $fixture = New-Fixture "conflict-$kind"
            $destination = Join-Path $fixture.CodexHome $(if ($kind -eq 'override') { 'AGENTS.override.md' } else { 'AGENTS.md' })
            if ($kind -eq 'link') {
                $foreign = Join-Path (Split-Path $fixture.SourceRoot) 'foreign.md'
                Write-FixtureFile $foreign 'foreign link target'
                New-Item -ItemType Directory -Path $fixture.CodexHome -Force | Out-Null
                New-Item -ItemType SymbolicLink -Path $destination -Target $foreign | Out-Null
            } else { Write-FixtureFile $destination 'foreign contents' }
            Assert-Throw { Invoke-HarnessInstall @fixture -Preview } 'Target conflict|AGENTS.override.md'
            Assert-Throw { Invoke-HarnessInstall @fixture } 'Target conflict|AGENTS.override.md'
            Assert-True ((Get-Content -LiteralPath $destination -Raw).Contains('foreign')) 'Conflicting contents must remain readable.'
            Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness'))) 'Conflict must fail before activation.'
        }
    }

    Test-Case 'missing original CLI and OpenSpec prerequisite fail before activation' {
        $fixture = New-Fixture 'missing-cli'
        Reset-TestModule -RealRuntime
        $fixture.CodexCommand = Join-Path (Split-Path $fixture.SourceRoot) 'missing-codex.ps1'
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Original Codex command unavailable'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Missing original CLI must not create host state.'
        $fixture = New-Fixture 'missing-openspec'
        & $testModule {
            function script:Get-Command {
                [CmdletBinding()]
                param([string] $Name)
                if ($Name -eq 'openspec') { return $null }
                Microsoft.PowerShell.Core\Get-Command @PSBoundParameters
            }
        }
        Assert-Throw { Invoke-HarnessInstall @fixture } 'OpenSpec CLI is required'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Missing OpenSpec must not create host state.'
    }

    Test-Case 'unsupported CLI version and missing file-profile help contract are rejected' {
        foreach ($kind in 'version', 'contract') {
            $fixture = New-Fixture "unsupported-$kind"
            Reset-TestModule -RealRuntime
            $fixture.CodexCommand = Join-Path (Split-Path $fixture.SourceRoot) 'unsupported-codex.ps1'
            $version = if ($kind -eq 'version') { '0.1.0' } else { '0.153.4' }
            Write-FixtureFile $fixture.CodexCommand @"
`$global:LASTEXITCODE = 0
if (`$args[0] -eq '--version') { 'codex-cli $version' } else { 'Legacy CLI help without a file-profile selector' }
"@
            Assert-Throw { Invoke-HarnessInstall @fixture } 'Codex .+ is required|required file-profile contract'
            Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Unsupported CLI must fail before host mutation.'
        }
    }

    Test-Case 'missing required source and foreign profile filename fail before activation' {
        $fixture = New-Fixture 'missing-source'
        Remove-Item -LiteralPath (Join-Path $fixture.SourceRoot 'tools/launcher.psm1')
        Assert-Throw { Invoke-HarnessInstall @fixture -Preview } 'Missing kit source'
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Missing kit source'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Missing source must not create host state.'
        $fixture = New-Fixture 'profile-name-conflict'
        $fixtureProfile = Join-Path $fixture.CodexHome 'harness.config.toml'
        Write-FixtureFile $fixtureProfile '# existing foreign profile'
        Assert-Throw { Invoke-HarnessInstall @fixture -Preview } 'Target conflict'
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Target conflict'
        Assert-True ((Get-Content -LiteralPath $fixtureProfile -Raw).Contains('existing foreign profile')) 'Foreign profile must not be overwritten.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness'))) 'Profile conflict must not start activation.'
    }

    Test-Case 'unavailable symbolic-link privilege fails with recovery guidance and no copies' {
        $fixture = New-Fixture 'link-privilege'
        & $testModule {
            function script:New-Item {
                [CmdletBinding()]
                param([string] $ItemType, [string] $Path, [string] $Target)
                if ($ItemType -eq 'SymbolicLink') { throw [UnauthorizedAccessException]::new('Injected: required symbolic-link privilege is not held.') }
                Microsoft.PowerShell.Management\New-Item @PSBoundParameters
            }
        }
        $before = $env:Path
        Assert-Throw { Invoke-HarnessInstall @fixture } 'required symbolic-link privilege.+Developer Mode/link privilege; no copy fallback'
        Assert-True ($env:Path -ceq $before) 'Unavailable link privilege must leave PATH unchanged.'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Failed link creation must remove its new empty host directories.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.SourceRoot 'global/principles-of-work.md')) 'Link failure must preserve its source.'
    }

    Test-Case 'native malformed shared profile rolls back without a committed registration' {
        $fixture = New-Fixture 'malformed-profile'
        Reset-TestModule -RealRuntime
        Write-FixtureFile (Join-Path $fixture.SourceRoot 'global/harness.config.toml') 'approval_policy = ['
        $before = $env:Path
        Assert-Throw { Invoke-HarnessInstall @fixture } 'prior connections restored: Codex startup failed'
        Assert-True ($env:Path -ceq $before) 'Native profile failure must restore PATH.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json'))) 'Invalid profile must not commit state.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness.config.toml'))) 'Invalid profile must not leave a deployed copy or link.'
    }

    Test-Case 'foreign capability names are rejected under alternate filenames' {
        $fixture = New-Fixture 'skill-name-collision'
        $firstSkill = Get-ChildItem -LiteralPath (Join-Path $fixture.SourceRoot '.agents/skills') -Directory | Select-Object -First 1
        $name = ([regex]::Match((Get-Content -LiteralPath (Join-Path $firstSkill.FullName 'SKILL.md') -Raw), '(?m)^name:\s*(.+)$')).Groups[1].Value.Trim()
        Write-FixtureFile (Join-Path $fixture.UserHome '.agents/skills/another-name/SKILL.md') "---`nname: $name`ndescription: foreign`n---"
        Assert-Throw { Invoke-HarnessInstall @fixture } 'name collision'
        $fixture = New-Fixture 'agent-name-collision'
        Write-FixtureFile (Join-Path $fixture.SourceRoot 'global/agents/repo.toml') "name = 'collision-agent'`ndescription = 'repository fixture'`ndeveloper_instructions = 'fixture'"
        Write-FixtureFile (Join-Path $fixture.CodexHome 'agents/personal.toml') "name = 'collision-agent'`ndescription = 'foreign fixture'`ndeveloper_instructions = 'foreign'"
        Assert-Throw { Invoke-HarnessInstall @fixture } 'name collision'
        Remove-Item -LiteralPath (Join-Path $fixture.CodexHome 'agents/personal.toml')
        Write-FixtureFile (Join-Path $fixture.CodexHome 'config.toml') "[agents.collision-agent]`ndescription = 'foreign local role'"
        Assert-Throw { Invoke-HarnessInstall @fixture } 'name collision'
        foreach ($inline in @("agents = { collision-agent = { description = 'foreign local role' } }", "[agents]`ncollision-agent = { description = 'foreign local role' }")) {
            Write-FixtureFile (Join-Path $fixture.CodexHome 'config.toml') $inline
            Assert-Throw { Invoke-HarnessInstall @fixture } 'name collision'
        }
    }

    Test-Case 'added and removed skill registrations reconcile while sources stay live' {
        $fixture = New-Fixture 'reconcile'
        Invoke-HarnessInstall @fixture | Out-Null
        $skill = Join-Path $fixture.SourceRoot '.agents/skills/lifecycle-fixture'
        Write-FixtureFile (Join-Path $skill 'SKILL.md') "---`nname: lifecycle-fixture`ndescription: fixture`n---`nversion one"
        Invoke-HarnessInstall @fixture | Out-Null
        $destination = Join-Path $fixture.UserHome '.agents/skills/lifecycle-fixture'
        Assert-True ((Get-Item -LiteralPath $destination).LinkType -eq 'SymbolicLink') 'New skill must be linked.'
        Add-Content -LiteralPath (Join-Path $skill 'SKILL.md') -Value 'version two'
        Assert-True ((Get-Content -LiteralPath (Join-Path $destination 'SKILL.md') -Raw).Contains('version two')) 'Connected source changes must be live.'
        Remove-Item -LiteralPath (Join-Path $skill 'SKILL.md')
        Remove-Item -LiteralPath $skill
        Invoke-HarnessInstall @fixture | Out-Null
        Assert-True (-not [bool](Get-Item -LiteralPath $destination -Force -ErrorAction SilentlyContinue)) 'Removed source must remove its obsolete link.'
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
    }

    Test-Case 'moved checkout is detected and reconnected' {
        $fixture = New-Fixture 'relocate'
        Invoke-HarnessInstall @fixture | Out-Null
        $oldSource = $fixture.SourceRoot
        $newSource = Join-Path (Split-Path $oldSource) 'relocated источник'
        foreach ($target in @($oldSource, $newSource)) {
            Assert-True ([IO.Path]::GetFullPath($target).StartsWith([IO.Path]::GetFullPath($suiteRoot) + '\', [StringComparison]::OrdinalIgnoreCase)) 'Move must stay inside fixture suite.'
        }
        Move-Item -LiteralPath $oldSource -Destination $newSource
        $fixture.SourceRoot = $newSource
        Assert-Throw { Invoke-HarnessInstall @fixture -Mode Check } 'Source unavailable|Broken'
        Invoke-HarnessInstall @fixture | Out-Null
        $state = Read-State $fixture
        Assert-True ($state.sourceRoot -eq $newSource) 'Metadata must refer to the relocated checkout.'
        Assert-DiagnosticLink $fixture
        Assert-True (@($state.links | Where-Object { $_.source.StartsWith($oldSource) }).Count -eq 0) 'Old source paths must be gone.'
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
    }

    Test-Case 'externally replaced managed target is preserved on disconnect' {
        $fixture = New-Fixture 'ownership'
        Invoke-HarnessInstall @fixture | Out-Null
        $destination = Join-Path $fixture.CodexHome 'AGENTS.md'
        Remove-Item -LiteralPath $destination
        Write-FixtureFile $destination 'external replacement'
        Assert-Throw { Invoke-HarnessInstall @fixture -Mode Disconnect } 'Ownership mismatch'
        Assert-True ((Get-Content -LiteralPath $destination -Raw).Contains('external replacement')) 'External replacement must survive disconnect.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json')) 'Failed disconnect must preserve recovery state.'
    }

    Test-Case 'preexisting same-source instruction link is retained on disconnect' {
        $fixture = New-Fixture 'adoption'
        New-Item -ItemType Directory -Path $fixture.CodexHome -Force | Out-Null
        $destination = Join-Path $fixture.CodexHome 'AGENTS.md'
        New-Item -ItemType SymbolicLink -Path $destination -Target (Join-Path $fixture.SourceRoot 'global/principles-of-work.md') | Out-Null
        Invoke-HarnessInstall @fixture | Out-Null
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        Assert-True ((Get-Item -LiteralPath $destination).LinkType -eq 'SymbolicLink') 'Installer must retain a preexisting compatible link.'
    }

    Test-Case 'failed final consumer validation rolls back the entire first activation' {
        $fixture = New-Fixture 'runtime-failure'
        & $testModule { function script:Test-HarnessRuntime { throw 'injected consumer failure' } }
        $before = $env:Path
        Assert-Throw { Invoke-HarnessInstall @fixture } 'prior connections restored: injected consumer failure'
        Assert-True ($env:Path -ceq $before) 'Failure must restore PATH.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json'))) 'Failure must not commit installation state.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'AGENTS.md'))) 'Failure must remove its created instruction link.'
        foreach ($name in $verificationResources.Keys) {
            Assert-True (-not [bool](Get-Item -LiteralPath (Join-Path $fixture.UserHome ".agents/skills/$name") -Force -ErrorAction SilentlyContinue)) 'Failed first activation must remove each owned verification skill link.'
            foreach ($relative in $verificationResources[$name]) {
                Assert-True (Test-Path -LiteralPath (Join-Path $fixture.SourceRoot ".agents/skills/$name/$relative") -PathType Leaf) 'Failed first activation must retain verification source resources.'
            }
        }
    }

    Test-Case 'failure after earlier link mutations restores existing activation' {
        $fixture = New-Fixture 'reconnect-failure'
        Invoke-HarnessInstall @fixture | Out-Null
        $previous = Read-State $fixture
        $originalSource = $fixture.SourceRoot
        $replacement = New-Fixture 'replacement-source'
        foreach ($name in $verificationResources.Keys) {
            foreach ($relative in $verificationResources[$name]) {
                Add-Content -LiteralPath (Join-Path $originalSource ".agents/skills/$name/$relative") -Value '# original resource revision'
                Add-Content -LiteralPath (Join-Path $replacement.SourceRoot ".agents/skills/$name/$relative") -Value '# replacement resource revision'
            }
        }
        Assert-VerificationResource $fixture '# original resource revision'
        $fixture.SourceRoot = $replacement.SourceRoot
        & $testModule { function script:Test-HarnessRuntime { throw 'injected reconnect failure' } }
        Assert-Throw { Invoke-HarnessInstall @fixture } 'prior connections restored: injected reconnect failure'
        Assert-True ((Read-State $fixture).sourceRoot -eq $originalSource) 'Rollback must restore the old metadata.'
        foreach ($link in $previous.links) { Assert-True ((Get-Item -LiteralPath $link.destination).Target -eq $link.source) 'Rollback must restore each previous target.' }
        $fixture.SourceRoot = $originalSource
        Assert-VerificationResource $fixture '# original resource revision'
        foreach ($name in $verificationResources.Keys) {
            foreach ($relative in $verificationResources[$name]) {
                Assert-True ((Get-Content -LiteralPath (Join-Path $replacement.SourceRoot ".agents/skills/$name/$relative") -Raw).Contains('# replacement resource revision')) 'Rollback must preserve replacement source resources too.'
            }
        }
    }

    Test-Case 'valid interrupted transaction can be recovered' {
        $fixture = New-Fixture 'recover'
        $before = $env:Path
        Invoke-HarnessInstall @fixture | Out-Null
        $pending = Get-PendingFromInstalled $fixture $before
        Write-Pending $fixture $pending
        Assert-Throw { Invoke-HarnessInstall @fixture } 'interrupted installation'
        Invoke-HarnessInstall @fixture -Mode Recover | Out-Null
        Assert-True ($env:Path -ceq $before) 'Recovery must restore its PATH.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json'))) 'Recovery must restore missing previous metadata.'
        Assert-True (-not (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'AGENTS.md'))) 'Recovery must undo its links.'
    }

    Test-Case 'tampered recovery destinations and sources are rejected before mutation' {
        foreach ($kind in 'destination', 'source') {
            $fixture = New-Fixture "recover-tamper-$kind"
            $before = $env:Path
            Invoke-HarnessInstall @fixture | Out-Null
            $pending = Get-PendingFromInstalled $fixture $before
            if ($kind -eq 'destination') { $pending.operations[0].destination = Join-Path (Split-Path $fixture.SourceRoot) 'foreign.md' }
            else { $pending.operations[0].newSource = Join-Path (Split-Path $fixture.SourceRoot) 'foreign-source.md' }
            Write-Pending $fixture $pending
            Assert-Throw { Invoke-HarnessInstall @fixture -Mode Recover } 'Unrecorded recovery destination|Recovery source is not owned'
            Assert-True (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'AGENTS.md')) 'Rejected recovery must not begin deleting connections.'
            $env:Path = $before
        }
    }

    Test-Case 'reparse destination parents and earlier command PATH entries are rejected' {
        $fixture = New-Fixture 'parent-link'
        $foreign = Join-Path (Split-Path $fixture.SourceRoot) 'foreign-directory'
        New-Item -ItemType Directory -Path $foreign, $fixture.UserHome -Force | Out-Null
        New-Item -ItemType SymbolicLink -Path (Join-Path $fixture.UserHome '.agents') -Target $foreign | Out-Null
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Parent directory is a reparse point'
        Assert-True (@(Get-ChildItem -LiteralPath $foreign -Force).Count -eq 0) 'Installer must not write through a foreign directory link.'
        $fixture = New-Fixture 'path-precedence'
        $earlier = Join-Path (Split-Path $fixture.SourceRoot) 'earlier command'
        Write-FixtureFile (Join-Path $earlier 'codex.cmd') '@exit /b 0'
        $bin = Join-Path $fixture.CodexHome 'harness/bin'
        Assert-Throw { & $testModule { param($pathValue, $entry) Assert-HarnessCommandPrecedence $pathValue $entry } ($earlier + ';' + $bin) $bin } 'Command precedence conflict'
        Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Precedence checks must be non-mutating.'
    }

    Test-Case 'rollback preserves concurrent unrelated PATH edit and records recovery' {
        $fixture = New-Fixture 'concurrent-path'
        & $testModule { function script:Test-HarnessRuntime { $env:Path += ';C:\harness-test-unrelated-path'; throw 'injected PATH race' } }
        Assert-Throw { Invoke-HarnessInstall @fixture } 'Recovery incomplete: PATH changed concurrently'
        Assert-True ($env:Path.EndsWith(';C:\harness-test-unrelated-path')) 'Unrelated PATH changes must survive rollback.'
        $pendingPath = Join-Path $fixture.CodexHome 'harness/pending.json'
        Assert-True (Test-Path -LiteralPath $pendingPath) 'Incomplete recovery must retain its journal.'
        $pending = Get-Content -LiteralPath $pendingPath -Raw | ConvertFrom-Json -AsHashtable
        $env:Path = $pending.pathBefore
        Invoke-HarnessInstall @fixture -Mode Recover | Out-Null
        Assert-True (-not (Test-Path -LiteralPath $pendingPath)) 'Resolved PATH conflict must allow recovery.'
    }

    Test-Case 'same-thread nested installation cannot corrupt the outer transaction' {
        $fixture = New-Fixture 'nested-install'
        & $testModule {
            param($arguments)
            $script:nestedArguments = $arguments
            $script:nestedMessage = $null
            $script:nestedStarted = $false
            $script:originalNewDirectory = (Get-Command New-HarnessDirectory).ScriptBlock
            function script:New-HarnessDirectory([string] $Path, $Created) {
                & $script:originalNewDirectory $Path $Created
                if (-not $script:nestedStarted -and $Path -eq (Join-Path $script:nestedArguments.CodexHome 'harness')) {
                    $script:nestedStarted = $true
                    try { Invoke-HarnessInstall @script:nestedArguments | Out-Null } catch { $script:nestedMessage = $_.Exception.Message }
                }
            }
        } $fixture
        Invoke-HarnessInstall @fixture | Out-Null
        $message = & $testModule { $script:nestedMessage }
        Assert-True ($message -match 'Another harness operation is active') 'Same-thread recursion must be rejected despite reentrant OS mutex.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'harness/installation.json')) 'Outer installer must retain its committed state.'
        Assert-True (Test-Path -LiteralPath (Join-Path $fixture.CodexHome 'AGENTS.md')) 'Outer installer must retain its instruction link.'
        Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
    }

    Test-Case 'separate process lock excludes concurrent operation before mutation' {
        $fixture = New-Fixture 'mutex'
        $ready = Join-Path (Split-Path $fixture.SourceRoot) 'lock.ready'
        $release = Join-Path (Split-Path $fixture.SourceRoot) 'lock.release'
        $start = [Diagnostics.ProcessStartInfo]::new($pwsh)
        $start.UseShellExecute = $false
        $start.CreateNoWindow = $true
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.Environment['HARNESS_TEST_USER'] = $fixture.UserHome
        $start.Environment['HARNESS_TEST_READY'] = $ready
        $start.Environment['HARNESS_TEST_RELEASE'] = $release
        $start.ArgumentList.Add('-NoProfile')
        $start.ArgumentList.Add('-Command')
        $start.ArgumentList.Add(@'
$ErrorActionPreference = 'Stop'
$identity = [Text.Encoding]::UTF8.GetBytes([IO.Path]::GetFullPath($env:HARNESS_TEST_USER).TrimEnd('\').ToLowerInvariant())
$mutex = [Threading.Mutex]::new($false, 'Local\CodexHarness-' + [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($identity)))
try {
    if (-not $mutex.WaitOne(0)) { throw 'Fixture lock unexpectedly occupied.' }
    Set-Content -LiteralPath $env:HARNESS_TEST_READY -Value ready
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not (Test-Path -LiteralPath $env:HARNESS_TEST_RELEASE) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 25 }
    $mutex.ReleaseMutex()
} finally { $mutex.Dispose() }
'@)
        $process = [Diagnostics.Process]::Start($start)
        try {
            $deadline = [DateTime]::UtcNow.AddSeconds(8)
            while (-not (Test-Path -LiteralPath $ready) -and -not $process.HasExited -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 25 }
            Assert-True (Test-Path -LiteralPath $ready) 'Separate process must acquire the installer mutex.'
            Assert-Throw { Invoke-HarnessInstall @fixture } 'Another harness operation is active'
            Assert-True (-not (Test-Path -LiteralPath $fixture.CodexHome)) 'Concurrent loser must not create host state.'
            Write-FixtureFile $release 'release'
            Assert-True ($process.WaitForExit(5000)) 'Lock fixture must exit promptly.'
            Assert-True ($process.ExitCode -eq 0) 'Lock fixture must release cleanly.'
            Invoke-HarnessInstall @fixture | Out-Null
            Invoke-HarnessInstall @fixture -Mode Disconnect | Out-Null
        } finally {
            if (-not $process.HasExited) { $process.Kill($true); $process.WaitForExit() }
            $process.Dispose()
        }
    }

    if ($failures.Count) { throw "Installer checks failed ($($failures.Count)):`n$($failures -join "`n")" }
    Write-Output "Installer checks passed ($cases lifecycle scenarios; $assertions assertions; real isolated install/Check/disconnect plus injected failure and recovery checks)."
} finally {
    $env:Path = $initialPath
    $resolved = [IO.Path]::GetFullPath($suiteRoot)
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $resolved.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetFileName($resolved) -notlike 'codex-installer проба *') { throw 'Unsafe installer fixture cleanup root.' }
    Remove-FixtureTree $resolved
}
